//! Pure, Host-neutral display of the supplied materialised snapshot.

use std::fmt::Write;

use receipts_orchestration::GraphSnapshot;

/// Renders snapshot evidence in stored node order with a trailing newline.
/// Text is quoted and display-escaped; absent optional values are `null`.
/// Summary is deliberately omitted; no topology is available or inferred.
/// This performs no observation, model call, or live Host presentation.
pub fn render_graph_snapshot(snapshot: &GraphSnapshot) -> String {
    let mut output = format!(
        "graph_id={}\ngraph_version={}\ncaptured_at={}\n",
        display_text(Some(snapshot.graph_id())),
        display_text(Some(snapshot.graph_version().as_str())),
        display_text(Some(snapshot.captured_at().as_str())),
    );
    for (index, node) in snapshot.node_states().iter().enumerate() {
        writeln!(
            output,
            "\nnode[{index}].node_id={}\nnode[{index}].state={}\nnode[{index}].kind={}\nnode[{index}].locked={}\nnode[{index}].code_sha={}",
            display_text(Some(node.node_id())),
            display_text(Some(node.state().as_str())),
            display_text(node.kind().map(|kind| kind.as_str())),
            match node.locked() {
                Some(true) => "true",
                Some(false) => "false",
                None => "null",
            },
            display_text(node.code_sha()),
        )
        .expect("writing to a String cannot fail");
    }
    writeln!(
        output,
        "\nresulting_digest={}",
        display_text(snapshot.resulting_digest()),
    )
    .expect("writing to a String cannot fail");
    output
}

// Lowercase hexadecimal escapes are display syntax, not JSON serialization.
fn display_text(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "null".to_owned();
    };
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control
                if control.is_control()
                    || matches!(
                        control,
                        '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
                    ) =>
            {
                write!(output, "\\u{{{:x}}}", u32::from(control))
                    .expect("writing to a String cannot fail");
            }
            printable => output.push(printable),
        }
    }
    output.push('"');
    output
}
