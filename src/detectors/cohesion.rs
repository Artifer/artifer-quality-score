//! LCOM4: how many unrelated responsibilities a class actually holds.
//!
//! Methods are connected when they share an instance field or one calls the other, and
//! the metric is the number of connected components.

use super::*;

/// The methods belonging to one class.
///
/// The *nearest* enclosing class, not any enclosing class. A nested class's methods are
/// its own, and a Ruby namespace (`module Billing` wrapping a class) would otherwise be
/// judged for every method in the file as though it had written them itself.
pub(crate) fn class_methods<'a, 'tree>(
    class_node: &Node,
    ctx: &'a FileContext<'tree, '_>,
) -> Vec<&'a FunctionContext<'tree>> {
    ctx.functions
        .iter()
        .filter(|f| enclosing_class(&f.node, ctx).is_some_and(|c| c.id() == class_node.id()))
        .collect()
}

/// The innermost class-like node a function sits in.
fn enclosing_class<'tree>(node: &Node<'tree>, ctx: &FileContext<'tree, '_>) -> Option<Node<'tree>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if metrics::is_kind(&candidate, &ctx.spec.class_nodes) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
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

/// Instance fields a method touches (`self.x`, `this.x`, `@x`).
pub(crate) fn self_attributes(f: &FunctionContext, ctx: &FileContext) -> HashSet<String> {
    let mut fields = HashSet::new();
    for node in metrics::own_nodes(&f.node, ctx.spec, ctx.source) {
        // Ruby names instance state in one node, with no receiver to check.
        if metrics::is_kind(&node, &ctx.spec.instance_field_nodes) {
            let name = metrics::text(&node, ctx.source).trim_start_matches('@');
            fields.insert(name.to_string());
            continue;
        }
        if !metrics::FIELD_ACCESS_NODES.contains(&node.kind()) {
            continue;
        }
        let object = node
            .child_by_field_name("object")
            .or_else(|| node.child_by_field_name("value"));
        let Some(object) = object else { continue };
        // The same construct under six names, with six sets of field names.
        let attribute = node
            .child_by_field_name("attribute")
            .or_else(|| node.child_by_field_name("property"))
            .or_else(|| node.child_by_field_name("field"))
            .or_else(|| node.child_by_field_name("name"));
        let Some(attribute) = attribute else { continue };
        let receiver = metrics::text(&object, ctx.source);
        if metrics::SELF_RECEIVERS.contains(&receiver) {
            fields.insert(metrics::text(&attribute, ctx.source).to_string());
        }
    }
    fields
}

/// Sibling methods a function calls on itself.
///
/// Where a self-call carries a receiver (`self.helper()`, `this.helper()`) it is already
/// indistinguishable from a field read, and `self_attributes` has it. Ruby calls a
/// sibling by bare name, so those have to be collected separately -- and kept *out* of
/// the field set, where two methods both calling `puts` would read as shared state.
pub(crate) fn self_calls(f: &FunctionContext, ctx: &FileContext) -> HashSet<String> {
    if !ctx.spec.implicit_self_calls {
        return self_attributes(f, ctx);
    }
    let nodes = metrics::own_nodes(&f.node, ctx.spec, ctx.source);
    // Names that belong to somebody else: `other.format_line` is not a call on self.
    let foreign: HashSet<usize> = nodes
        .iter()
        .filter(|node| node.kind() == "call")
        .filter(|node| match node.child_by_field_name("receiver") {
            None => false,
            Some(receiver) => metrics::text(&receiver, ctx.source) != "self",
        })
        .filter_map(|node| node.child_by_field_name("method"))
        .map(|name| name.id())
        .collect();

    let mut called = self_attributes(f, ctx);
    for node in &nodes {
        // Every other bare name is a call on self. Ruby writes `format_line(n)`,
        // `count_orders` and a local variable identically, and only names matching a
        // sibling method are ever used -- so a local that shares one costs nothing.
        if node.kind() == "identifier" && !foreign.contains(&node.id()) {
            called.insert(metrics::text(node, ctx.source).to_string());
        }
    }
    called
}

/// The names a class stores state under: every `@field` anywhere in its body.
///
/// Ruby reads a field through a generated accessor as often as through `@options`
/// directly -- `attr_reader :options`, and from then on a bare `options` -- and the two
/// are the same field. Without this, a class whose methods all go through its own
/// accessors shares no state at all as far as LCOM4 can see, and every method reads as a
/// separate responsibility. The set is empty for languages that spell a field access
/// out, which makes the accessor step below a no-op there.
fn declared_fields(class_node: &Node, ctx: &FileContext) -> HashSet<String> {
    let mut names = HashSet::new();
    for node in metrics::all_nodes(class_node) {
        // Ruby: every `@field` the class mentions, which is where the name lives.
        if metrics::is_kind(&node, &ctx.spec.instance_field_nodes) {
            names.insert(field_name(metrics::text(&node, ctx.source)));
            continue;
        }
        // Java: a `field_declaration`, which names one field per declarator.
        if metrics::is_kind(&node, &ctx.spec.field_declaration_nodes) {
            names.extend(declared_names(&node, ctx));
        }
    }
    names
}

/// A field's name without the sigil its language writes it with: Ruby's `@amount` and
/// PHP's `$amount` are read back as a bare `amount`.
fn field_name(raw: &str) -> String {
    raw.trim_start_matches(['@', '$']).to_string()
}

/// The names one field declaration introduces. `private String a, b;` declares two.
fn declared_names(declaration: &Node, ctx: &FileContext) -> Vec<String> {
    let named = |node: &Node| {
        node.child_by_field_name("name")
            .map(|name| field_name(metrics::text(&name, ctx.source)))
    };
    if let Some(name) = named(declaration) {
        return vec![name];
    }
    metrics::all_nodes(declaration).iter().filter_map(named).collect()
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
    declared: &HashSet<String>,
    ctx: &FileContext,
) -> usize {
    let calls: HashMap<&str, HashSet<String>> = connecting
        .iter()
        .map(|f| (f.name.as_str(), self_calls(f, ctx)))
        .collect();
    let fields: HashMap<&str, HashSet<String>> = connecting
        .iter()
        .map(|f| {
            let mut touched = self_attributes(f, ctx);
            // A bare name that matches one of the class's fields is that field, read
            // through its accessor.
            touched.extend(calls[f.name.as_str()].intersection(declared).cloned());
            (f.name.as_str(), touched)
        })
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
        for called in calls[f.name.as_str()].iter().filter(|n| names.contains(n.as_str())) {
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
                let is_namespaced = ctx
                    .spec
                    .namespace_function
                    .is_some_and(|is_namespace| is_namespace(&f.node, ctx.source));
                let has_instance = !ctx.spec.explicit_receiver
                    || metrics::has_receiver(&f.node, ctx.spec, ctx.source);
                has_instance && !is_namespaced && !f.is_constructor
            })
            .collect();
        let declared = declared_fields(&class_node, ctx);
        // Accessors connect fields but are not responsibilities of their own.
        let methods: Vec<&FunctionContext> = instance_methods
            .iter()
            .copied()
            .filter(|f| !metrics::is_pure_accessor(&f.node, ctx.spec, ctx.source, &declared))
            .collect();
        if methods.len() < 2 {
            continue;
        }

        let components = lcom4(&methods, &instance_methods, &declared, ctx);
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

