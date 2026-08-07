//! LCOM4: how many unrelated responsibilities a class actually holds.
//!
//! Methods are connected when they share an instance field or one calls the other, and
//! the metric is the number of connected components.

use super::*;

pub(crate) fn class_methods<'a, 'tree>(
    class_node: &Node,
    ctx: &'a FileContext<'tree, '_>,
) -> Vec<&'a FunctionContext<'tree>> {
    ctx.functions
        .iter()
        .filter(|f| {
            f.node.start_byte() >= class_node.start_byte()
                && f.node.end_byte() <= class_node.end_byte()
                && f.node.id() != class_node.id()
        })
        .collect()
}

/// A large, many-method class that also contains a Brain Method.
pub(crate) fn brain_class(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let loc_threshold = ctx.size_threshold("class_max_lines_of_code");
    let fn_threshold = ctx.size_threshold("class_max_functions");
    let brain_loc = ctx.threshold("brain_method_lines_of_code");
    let brain_cc = ctx.threshold("brain_method_cyclomatic_complexity");
    let brain_nesting = ctx.threshold("brain_method_nesting_depth");

    for class_node in metrics::class_nodes(&ctx.root, ctx.spec) {
        let methods = class_methods(&class_node, ctx);
        let loc = metrics::logical_lines(&class_node);
        let has_brain_method = methods.iter().any(|f| {
            (f.loc as f64) > brain_loc
                && (f.complexity as f64) > brain_cc
                && (f.nesting as f64) > brain_nesting
        });
        let is_brain_class =
            (loc as f64) > loc_threshold && (methods.len() as f64) > fn_threshold;
        if is_brain_class && has_brain_method {
            let name = metrics::class_name(&class_node, ctx.source)
                .unwrap_or_else(|| "<anonymous class>".to_string());
            out.push(RawFinding {
                category: prof::BRAIN_CLASS,
                details: format!(
                    "LoC = {} lines, {} functions, contains a Brain Method",
                    loc,
                    methods.len()
                ),
                range: Range::line(class_node.start_position().row + 1),
                value: loc as f64,
                threshold: loc_threshold,
                function: Some(name),
            });
        }
    }
}

/// Instance fields a method touches (`self.x` / `this.x`).
pub(crate) fn self_attributes(f: &FunctionContext, ctx: &FileContext) -> HashSet<String> {
    let mut fields = HashSet::new();
    for node in metrics::own_nodes(&f.node, ctx.spec, ctx.source) {
        // Python `attribute`, TS/JS `member_expression`, Rust `field_expression` — the
        // same construct under three names, with three sets of field names.
        if !matches!(node.kind(), "attribute" | "member_expression" | "field_expression") {
            continue;
        }
        let object = node
            .child_by_field_name("object")
            .or_else(|| node.child_by_field_name("value"));
        let Some(object) = object else { continue };
        let attribute = node
            .child_by_field_name("attribute")
            .or_else(|| node.child_by_field_name("property"))
            .or_else(|| node.child_by_field_name("field"));
        let Some(attribute) = attribute else { continue };
        let receiver = metrics::text(&object, ctx.source);
        if receiver == "self" || receiver == "this" {
            fields.insert(metrics::text(&attribute, ctx.source).to_string());
        }
    }
    fields
}

/// Disjoint-set over method names, for counting connected components.
struct Components {
    parent: HashMap<String, String>,
}

impl Components {
    fn new(names: impl IntoIterator<Item = String>) -> Self {
        let parent = names.into_iter().map(|n| (n.clone(), n)).collect();
        Self { parent }
    }

    fn find(&mut self, name: &str) -> String {
        let mut current = name.to_string();
        loop {
            let next = self.parent[&current].clone();
            if next == current {
                return current;
            }
            let grandparent = self.parent[&next].clone();
            self.parent.insert(current, grandparent);
            current = next;
        }
    }

    fn union(&mut self, left: &str, right: &str) {
        let (left_root, right_root) = (self.find(left), self.find(right));
        if left_root != right_root {
            self.parent.insert(right_root, left_root);
        }
    }

    fn count<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) -> usize {
        let roots: HashSet<String> = names.into_iter().map(|n| self.find(n)).collect();
        roots.len()
    }
}

/// LCOM4 for one class: methods are connected when they share an instance field or one
/// calls the other, and the metric is the number of connected components.
///
/// Two roles, deliberately separated. `counted` methods are the ones a component count
/// is taken over; `connecting` additionally includes pure accessors, because an accessor
/// reading two fields is real evidence that those fields belong together. Excluding an
/// accessor from connectivity as well as from counting could *raise* LCOM4 by removing
/// the very link that held two methods in one component — the opposite of what an
/// exemption should do.
///
/// Constructors stay out of both: they touch every field, so including them would
/// collapse any class into a single component and the metric would never fire.
fn lcom4(
    counted: &[&FunctionContext],
    connecting: &[&FunctionContext],
    ctx: &FileContext,
) -> usize {
    let fields: HashMap<&str, HashSet<String>> = connecting
        .iter()
        .map(|f| (f.name.as_str(), self_attributes(f, ctx)))
        .collect();
    let names: HashSet<&str> = connecting.iter().map(|f| f.name.as_str()).collect();
    let mut components = Components::new(connecting.iter().map(|f| f.name.clone()));

    for (index, f) in connecting.iter().enumerate() {
        // Shared instance state connects two methods.
        for other in connecting.iter().skip(index + 1) {
            if !fields[f.name.as_str()].is_disjoint(&fields[other.name.as_str()]) {
                components.union(&f.name, &other.name);
            }
        }
        // So does one calling the other.
        for called in fields[f.name.as_str()].iter().filter(|n| names.contains(n.as_str())) {
            components.union(&f.name, called);
        }
    }
    components.count(counted.iter().map(|f| f.name.as_str()))
}

/// A class whose methods form several unrelated groups.
///
/// Constructors touch every field and pure accessors touch exactly one, so neither says
/// anything about how the class is divided; both are excluded, as are interfaces.
pub(crate) fn low_cohesion(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.threshold("class_max_lcom4");

    for class_node in metrics::class_nodes(&ctx.root, ctx.spec) {
        if metrics::is_abstract_class(&class_node, ctx.spec, ctx.source) {
            continue;
        }
        // Everything with an instance to speak of, minus constructors.
        let instance_methods: Vec<&FunctionContext> = class_methods(&class_node, ctx)
            .into_iter()
            .filter(|f| {
                let has_instance = !ctx.spec.explicit_receiver
                    || metrics::has_receiver(&f.node, ctx.spec, ctx.source);
                has_instance && !f.is_constructor
            })
            .collect();
        // Accessors connect fields but are not responsibilities of their own.
        let methods: Vec<&FunctionContext> = instance_methods
            .iter()
            .copied()
            .filter(|f| !metrics::is_pure_accessor(&f.node, ctx.spec, ctx.source))
            .collect();
        if methods.len() < 2 {
            continue;
        }

        let components = lcom4(&methods, &instance_methods, ctx);
        if (components as f64) <= threshold {
            continue;
        }
        let name = metrics::class_name(&class_node, ctx.source)
            .unwrap_or_else(|| "<anonymous class>".to_string());
        out.push(RawFinding {
            category: prof::LOW_COHESION,
            details: format!(
                "LCOM4 = {} unrelated responsibilities across {} functions",
                components,
                methods.len()
            ),
            range: Range::line(class_node.start_position().row + 1),
            value: components as f64,
            threshold,
            function: Some(name),
        });
    }
}

