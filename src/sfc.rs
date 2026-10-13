//! Single-file components: Vue and Svelte.
//!
//! An SFC is not a language of its own: it is an HTML wrapper holding a `<script>`
//! block written in JavaScript or TypeScript, markup, and a `<style>`. aqs scores the
//! script — the part written in a language it parses — and leaves the other two to the
//! tools that own them (eslint-plugin-vue, svelte-check, stylelint). Neither framework
//! needs a grammar of its own for this: the script inside is ordinary JS or TS.
//!
//! The script is extracted **in place**: every line outside it is blanked, keeping
//! its newline, so tree-sitter's rows still line up with the `.vue` file and a
//! finding points at the line a reader will actually open.
//!
//! Template and style lines are not counted as LoC either. A line this tool cannot
//! parse is a line it cannot explain a finding about, and an unexplainable number is
//! worse than an absent one. If markup is ever scored, it needs metrics of its own
//! rather than a line count borrowed from the script's rules.

use tree_sitter::Node;

/// Options of the default export whose functions describe the component rather
/// than doing anything.
///
/// A one-line `computed` getter, a prop's `validator`, the `data()` seed: these
/// are declarations. Counting them as functions is what makes a wiring
/// container — thirty getters forwarding to the store, in 130 lines — look like
/// a god component, while saying nothing about how much behaviour a reader has
/// to hold. What runs still counts: `methods`, `watch` handlers, `setup`, and
/// the lifecycle hooks.
const DECLARATIVE_OPTIONS: &[&str] = &[
    "props",
    "computed",
    "data",
    "components",
    "emits",
    "mixins",
    "inject",
];

/// What an SFC's script turned out to be.
pub struct Script {
    /// The file with everything but the script blanked out, line for line.
    pub source: Vec<u8>,
    /// `javascript`, or `typescript` when a block declares `lang="ts"`.
    pub language: &'static str,
}

/// Whether this path is a single-file component whose script must be extracted.
pub fn is_component(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    lowered.ends_with(".vue") || lowered.ends_with(".svelte")
}

/// Whether this path is specifically a Vue SFC.
///
/// Narrower than `is_component` on purpose: only Vue has the options object that
/// `declarative_ranges` reads, and asking a Svelte component for one would be asking a
/// question its syntax does not answer.
pub fn is_vue(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".vue")
}

/// Whether an opening `<script>` tag declares TypeScript.
fn declares_typescript(tag: &str) -> bool {
    ["lang=\"ts\"", "lang='ts'", "lang=\"tsx\"", "lang='tsx'"]
        .iter()
        .any(|marker| tag.contains(marker))
}

/// The line's ending, and nothing else — what a blanked line leaves behind.
fn blanked(line: &str) -> &str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

/// The script blocks of an SFC, blanked in place.
///
/// Every block is kept: Vue's `<script>` + `<script setup>` pair and Svelte's
/// `<script>` + `<script module>` pair are each two halves of one component, and
/// either half can hold the code worth scoring. A component with no
/// script at all yields an empty file, which scores nothing and weighs nothing.
///
/// The tag lines themselves are blanked — they are HTML, not JavaScript — so a block
/// opened and closed on a single line contributes no code. That is a real limitation
/// and a deliberate one: recognising `<script>` only at the start of a line is what
/// keeps the scan from running away on a `<script` that appears in template text,
/// and a one-line script block does not occur in a formatted SFC.
pub fn extract(source: &[u8]) -> Script {
    let text = String::from_utf8_lossy(source);
    let mut out = String::with_capacity(text.len());
    let mut language = "javascript";
    let mut inside = false;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();

        if !inside && trimmed.starts_with("<script") {
            if declares_typescript(line) {
                language = "typescript";
            }
            // A block that also closes on this line never opens.
            inside = !trimmed.contains("</script");
            out.push_str(blanked(line));
            continue;
        }

        if inside && trimmed.starts_with("</script") {
            inside = false;
            out.push_str(blanked(line));
            continue;
        }

        out.push_str(if inside { line } else { blanked(line) });
    }

    Script {
        source: out.into_bytes(),
        language,
    }
}

/// The object of the component's `export default`, if it has one.
fn default_export_object<'tree>(root: &Node<'tree>) -> Option<Node<'tree>> {
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        if node.kind() == "export_statement" {
            let mut cursor = node.walk();
            let object = node.children(&mut cursor).find(|c| c.kind() == "object");
            if object.is_some() {
                return object;
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    None
}

/// The name an option is written under, for either spelling a component uses:
/// `computed: { … }` (a pair) or `data() { … }` (a method).
fn option_name<'src>(node: &Node, source: &'src [u8]) -> Option<&'src str> {
    if !matches!(node.kind(), "pair" | "method_definition") {
        return None;
    }
    let key = node.named_child(0)?;

    std::str::from_utf8(&source[key.start_byte()..key.end_byte()]).ok()
}

/// Byte ranges of the component options that only describe it.
///
/// Empty for anything that is not a component with a default export, so the
/// caller needs no special case for a plain script or a `<script setup>`.
pub fn declarative_ranges(root: &Node, source: &[u8]) -> Vec<(usize, usize)> {
    let Some(object) = default_export_object(root) else {
        return Vec::new();
    };

    let mut cursor = object.walk();

    object
        .named_children(&mut cursor)
        .filter(|option| {
            option_name(option, source).is_some_and(|name| DECLARATIVE_OPTIONS.contains(&name))
        })
        .map(|option| (option.start_byte(), option.end_byte()))
        .collect()
}

/// Whether `byte` falls inside one of `ranges`.
pub fn within(ranges: &[(usize, usize)], byte: usize) -> bool {
    ranges
        .iter()
        .any(|(start, end)| byte >= *start && byte < *end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_last_line_without_a_newline_is_blanked_to_nothing() {
        assert_eq!(extracted("<template>\n  <p>hi</p>\n</template>"), "\n\n");
    }

    #[test]
    fn a_spread_in_the_options_object_is_not_an_option() {
        let found =
            declarative("export default { ...base, props: { a: Number }, data() { return {} } }");
        assert_eq!(found, vec!["props", "data"]);
    }

    #[test]
    fn a_range_holds_its_first_byte_and_not_its_end() {
        let ranges = [(2, 5)];
        assert!(!within(&ranges, 1));
        assert!(within(&ranges, 2));
        assert!(within(&ranges, 4));
        assert!(!within(&ranges, 5));
    }

    fn extracted(source: &str) -> String {
        String::from_utf8(extract(source.as_bytes()).source).expect("utf-8")
    }

    fn language(source: &str) -> &'static str {
        extract(source.as_bytes()).language
    }

    const SFC: &str = "<template>\n    <p>{{ hello }}</p>\n</template>\n\n<script>\nexport default {\n    data: () => ({ hello: 'hi' }),\n}\n</script>\n\n<style>\np { color: red; }\n</style>\n";

    #[test]
    fn keeps_the_script_and_blanks_the_rest() {
        assert_eq!(
            extracted(SFC),
            "\n\n\n\n\nexport default {\n    data: () => ({ hello: 'hi' }),\n}\n\n\n\n\n\n"
        );
    }

    #[test]
    fn leaves_every_line_where_it_was() {
        let script = extracted(SFC);

        // `export default` is on line 6 of the SFC and must stay on line 6, or a
        // finding would point at the wrong line of the file a reader opens.
        assert_eq!(script.lines().nth(5), Some("export default {"));
        assert_eq!(script.lines().count(), SFC.lines().count());
    }

    #[test]
    fn reads_the_lang_attribute() {
        assert_eq!(language(SFC), "javascript");
        assert_eq!(
            language("<script lang=\"ts\">\nlet a: number = 1\n</script>\n"),
            "typescript"
        );
        assert_eq!(
            language("<script setup lang='ts'>\nlet a: number = 1\n</script>\n"),
            "typescript"
        );
        assert_eq!(
            language("<script setup>\nlet a = 1\n</script>\n"),
            "javascript"
        );
    }

    #[test]
    fn keeps_both_halves_of_a_script_setup_pair() {
        let source = "<script>\nexport default { name: 'A' }\n</script>\n\n<script setup>\nconst a = 1\n</script>\n";

        let script = extracted(source);

        assert!(script.contains("export default { name: 'A' }"));
        assert!(script.contains("const a = 1"));
    }

    #[test]
    fn takes_typescript_from_either_half() {
        let source = "<script lang=\"ts\">\nlet a: number = 1\n</script>\n<script setup>\nconst b = 2\n</script>\n";

        assert_eq!(language(source), "typescript");
    }

    #[test]
    fn a_component_with_no_script_holds_no_code() {
        let source = "<template>\n    <p>static</p>\n</template>\n";

        assert_eq!(extracted(source).trim(), "");
    }

    #[test]
    fn a_script_tag_in_the_template_does_not_swallow_the_file() {
        // The tag is indented, so it opens a block; what matters is that the
        // closing tag ends it again rather than the rest of the file being read
        // as JavaScript.
        let source = "<template>\n    <script>alert(1)</script>\n</template>\n<script>\nconst a = 1\n</script>\n";

        let script = extracted(source);

        assert!(script.contains("const a = 1"));
        assert!(!script.contains("<template>"));
    }

    #[test]
    fn recognises_the_extension_only_at_the_end() {
        assert!(is_vue("domains/x/Player.vue"));
        assert!(is_vue("Player.VUE"));
        assert!(!is_vue("vue/helper.js"));
        assert!(!is_vue("Player.vue.snap"));
    }

    /// Parse a script the way the scanner does, and hand back its root.
    fn parsed(script: &str) -> (tree_sitter::Tree, Vec<u8>) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .expect("grammar loads");
        let source = script.as_bytes().to_vec();
        let tree = parser.parse(&source, None).expect("parses");

        (tree, source)
    }

    /// The names of the options `declarative_ranges` covered, for readability.
    fn declarative(script: &str) -> Vec<String> {
        let (tree, source) = parsed(script);

        declarative_ranges(&tree.root_node(), &source)
            .into_iter()
            .map(|(start, _)| {
                let text = String::from_utf8_lossy(&source[start..]);
                text.split(['(', ':'])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
            .collect()
    }

    const COMPONENT: &str = "export default {
        name: 'A',
        props: { size: { validator: (v) => v > 0 } },
        data() { return { open: false } },
        computed: { total() { return 1 }, label() { return 'x' } },
        watch: { open: { handler() {} } },
        mounted() {},
        methods: { go() {}, stop() {} },
    }";

    #[test]
    fn covers_the_options_that_only_describe_the_component() {
        assert_eq!(declarative(COMPONENT), ["props", "data", "computed"]);
    }

    #[test]
    fn leaves_what_runs_outside_the_ranges() {
        let (tree, source) = parsed(COMPONENT);
        let ranges = declarative_ranges(&tree.root_node(), &source);

        // `methods`, `watch` and the lifecycle hooks are behaviour and must
        // stay countable; only the declarations are covered.
        for option in ["methods", "watch", "mounted"] {
            let at = COMPONENT.find(option).expect("option is in the fixture");
            assert!(!within(&ranges, at), "{option} should not be covered");
        }
        let computed = COMPONENT.find("total()").expect("getter is in the fixture");
        assert!(
            within(&ranges, computed),
            "a computed getter should be covered"
        );
    }

    #[test]
    fn covers_nothing_without_a_default_export() {
        // A `<script setup>` block, or a plain module: every function in it is
        // behaviour, so the count is unchanged.
        assert!(declarative("const a = 1\nfunction go() {}").is_empty());
        assert!(declarative("export const go = () => {}").is_empty());
    }

    #[test]
    fn keeps_windows_line_endings_intact() {
        let source =
            "<template>\r\n<p/>\r\n</template>\r\n<script>\r\nconst a = 1\r\n</script>\r\n";

        let script = extracted(source);

        assert_eq!(script.lines().count(), 6);
        assert_eq!(script.lines().nth(4), Some("const a = 1"));
    }

    #[test]
    fn a_svelte_component_keeps_both_of_its_script_blocks() {
        let source = "<script context=\"module\" lang=\"ts\">\nexport const ssr = true\n</script>\n\n<script lang=\"ts\">\nlet count = 0\n</script>\n\n<button on:click={() => count++}>{count}</button>\n\n<style>\n  button { color: red; }\n</style>\n";
        let script = extract(source.as_bytes());
        let out = String::from_utf8(script.source).expect("utf-8");

        assert_eq!(script.language, "typescript");
        assert!(out.contains("export const ssr = true"), "{out}");
        assert!(out.contains("let count = 0"), "{out}");
        // Markup and style are blanked, and the line count is preserved so a finding
        // still points at the line a reader will open.
        assert!(!out.contains("button {"), "{out}");
        assert!(!out.contains("on:click"), "{out}");
        assert_eq!(out.lines().count(), source.lines().count());
    }

    #[test]
    fn a_svelte_component_without_lang_is_javascript() {
        assert_eq!(language("<script>\nlet a = 1\n</script>\n"), "javascript");
    }

    #[test]
    fn only_vue_is_asked_for_an_options_object() {
        assert!(is_component("src/Counter.svelte"));
        assert!(is_component("src/Player.vue"));
        assert!(!is_component("src/main.ts"));
        assert!(is_vue("src/Player.vue"));
        assert!(!is_vue("src/Counter.svelte"));
    }
}
