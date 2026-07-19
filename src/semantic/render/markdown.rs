//! Semantic Markdown projection from a normalized [`SemanticDocument`].
//!
//! Walks the component tree only (no browser or HTML). Preserves opaque
//! `semantic_ref` markers where the projection contract includes them and
//! truncates under shared render budgets.

use crate::semantic::component::{SemanticComponent, SemanticKind};
use crate::semantic::document::SemanticDocument;
use crate::semantic::identity::SemanticRef;
use crate::semantic::render::bounds::{RenderError, RenderedOutput, bound_output};

/// Render the full document as human-readable semantic Markdown.
///
/// Preserves document order, interaction metadata expressible in Markdown, and
/// model-provided opaque refs (as HTML comments on addressable blocks). Does
/// not derive identities from display text.
pub fn render_semantic_markdown(
    document: &SemanticDocument,
) -> Result<RenderedOutput, RenderError> {
    let mut out = String::new();
    write_document_header(&mut out, document);
    for root in &document.roots {
        write_block(&mut out, root, 0);
    }
    trim_trailing_blank(&mut out);
    Ok(bound_output(
        out,
        &document.document.document_id,
        &document.document.revision,
    ))
}

/// Render one component (and its descendants) addressed by an exact `semantic_ref`.
///
/// Fail-closed: invalid, stale, unknown, or wrong-document refs return
/// [`RenderError::Ref`] without guessing a substitute.
pub fn render_component_markdown(
    document: &SemanticDocument,
    semantic_ref: &SemanticRef,
) -> Result<RenderedOutput, RenderError> {
    let component = document.resolve(semantic_ref)?;
    let mut out = String::new();
    write_document_header(&mut out, document);
    out.push_str("<!-- component_fragment -->\n\n");
    write_block(&mut out, component, 0);
    trim_trailing_blank(&mut out);
    Ok(bound_output(
        out,
        &document.document.document_id,
        &document.document.revision,
    ))
}

fn write_document_header(out: &mut String, document: &SemanticDocument) {
    let meta = &document.document;
    out.push_str(&format!(
        "<!-- semantic_document document_id=\"{}\" revision=\"{}\" url=\"{}\" title=\"{}\" -->\n\n",
        escape_attr(&meta.document_id),
        escape_attr(&meta.revision),
        escape_attr(&meta.url),
        escape_attr(&meta.title),
    ));
    if !meta.title.is_empty() {
        out.push_str("# ");
        out.push_str(&escape_inline(&meta.title));
        out.push_str("\n\n");
    }
}

fn write_block(out: &mut String, component: &SemanticComponent, list_depth: usize) {
    match component.kind {
        SemanticKind::Landmark => write_landmark(out, component, list_depth),
        SemanticKind::Heading => write_heading(out, component),
        SemanticKind::Text => write_text_block(out, component),
        SemanticKind::List => write_list(out, component, list_depth),
        SemanticKind::ListItem => {
            // List items are emitted by write_list with ordered context.
            write_list_item(out, component, list_depth, false, 1);
        }
        SemanticKind::Link => {
            push_ref_comment(out, component);
            out.push_str(&format_link(component));
            out.push_str("\n\n");
        }
        SemanticKind::Image => {
            push_ref_comment(out, component);
            out.push_str(&format_image(component));
            out.push_str("\n\n");
        }
        SemanticKind::Input
        | SemanticKind::Textarea
        | SemanticKind::Select
        | SemanticKind::Button => {
            push_ref_comment(out, component);
            out.push_str(&format_control(component));
            out.push_str("\n\n");
        }
        SemanticKind::Group => write_group(out, component, list_depth),
    }
}

fn write_landmark(out: &mut String, component: &SemanticComponent, list_depth: usize) {
    let role = landmark_role_name(component);
    out.push_str(&format!(
        "<!-- landmark role=\"{}\" semantic_ref=\"{}\" -->\n\n",
        escape_attr(&role),
        escape_attr(component.semantic_ref.as_str()),
    ));
    for child in &component.children {
        write_block(out, child, list_depth);
    }
    out.push_str(&format!(
        "<!-- /landmark role=\"{}\" -->\n\n",
        escape_attr(&role),
    ));
}

fn write_heading(out: &mut String, component: &SemanticComponent) {
    push_ref_comment(out, component);
    let level = component.attrs.heading_level.unwrap_or(2).clamp(1, 6);
    for _ in 0..level {
        out.push('#');
    }
    out.push(' ');
    if component.children.is_empty() {
        out.push_str(&escape_inline(display_text(component)));
    } else {
        write_inline_children(out, component);
    }
    out.push_str("\n\n");
}

fn write_text_block(out: &mut String, component: &SemanticComponent) {
    push_ref_comment(out, component);
    if component.children.is_empty() {
        if let Some(text) = component.text.as_deref() {
            out.push_str(&escape_inline(text));
            out.push_str("\n\n");
        }
    } else {
        // Mixed inline: walk children in document order; never replay aggregate text.
        write_inline_children(out, component);
        out.push_str("\n\n");
    }
}

fn write_list(out: &mut String, component: &SemanticComponent, list_depth: usize) {
    push_ref_comment(out, component);
    let ordered = component.attrs.ordered.unwrap_or(false);
    let mut index = 1usize;
    for child in &component.children {
        if child.kind == SemanticKind::ListItem {
            write_list_item(out, child, list_depth, ordered, index);
            index += 1;
        } else {
            write_block(out, child, list_depth);
        }
    }
    if list_depth == 0 {
        out.push('\n');
    }
}

fn write_list_item(
    out: &mut String,
    component: &SemanticComponent,
    list_depth: usize,
    ordered: bool,
    index: usize,
) {
    push_ref_comment(out, component);
    let indent = "  ".repeat(list_depth);
    if ordered {
        out.push_str(&format!("{indent}{index}. "));
    } else {
        out.push_str(&format!("{indent}- "));
    }

    if component.children.is_empty() {
        out.push_str(&escape_inline(display_text(component)));
        out.push('\n');
        return;
    }

    let mut nested_lists = Vec::new();
    let mut nested_blocks = Vec::new();
    let mut wrote_inline = false;
    for child in &component.children {
        match child.kind {
            SemanticKind::List => nested_lists.push(child),
            SemanticKind::Landmark | SemanticKind::Group | SemanticKind::Heading => {
                nested_blocks.push(child);
            }
            _ => {
                if wrote_inline {
                    maybe_join_space(out, child);
                }
                write_inline_piece(out, child);
                wrote_inline = true;
            }
        }
    }
    if !wrote_inline {
        out.push_str(&escape_inline(display_text(component)));
    }
    out.push('\n');

    for nested in nested_lists {
        write_list(out, nested, list_depth + 1);
    }
    for nested in nested_blocks {
        write_block(out, nested, list_depth + 1);
    }
}

fn write_group(out: &mut String, component: &SemanticComponent, list_depth: usize) {
    push_ref_comment(out, component);
    if let Some(label) = component.label.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("**");
        out.push_str(&escape_inline(label));
        out.push_str("**\n\n");
    }
    for child in &component.children {
        write_block(out, child, list_depth);
    }
}

fn write_inline_children(out: &mut String, component: &SemanticComponent) {
    let mut first = true;
    for child in &component.children {
        if !first {
            maybe_join_space(out, child);
        }
        write_inline_piece(out, child);
        first = false;
    }
}

fn write_inline_piece(out: &mut String, component: &SemanticComponent) {
    match component.kind {
        SemanticKind::Text => {
            if component.children.is_empty() {
                if let Some(text) = component.text.as_deref() {
                    out.push_str(&escape_inline(text));
                }
            } else {
                write_inline_children(out, component);
            }
        }
        SemanticKind::Link => {
            // Preserve model-provided opaque refs for inline links (never invent from text).
            push_inline_ref_comment(out, component);
            out.push_str(&format_link(component));
        }
        SemanticKind::Image => {
            push_inline_ref_comment(out, component);
            out.push_str(&format_image(component));
        }
        SemanticKind::Input
        | SemanticKind::Textarea
        | SemanticKind::Select
        | SemanticKind::Button => {
            push_inline_ref_comment(out, component);
            out.push_str(&format_control(component));
        }
        _ => out.push_str(&escape_inline(display_text(component))),
    }
}

fn push_inline_ref_comment(out: &mut String, component: &SemanticComponent) {
    out.push_str(&format!(
        "<!-- semantic_ref=\"{}\" kind=\"{}\" -->",
        escape_attr(component.semantic_ref.as_str()),
        escape_attr(kind_name(component.kind)),
    ));
}

fn maybe_join_space(out: &mut String, next: &SemanticComponent) {
    let need_space = match (out.chars().next_back(), first_display_char(next)) {
        (Some(a), Some(b)) if !a.is_whitespace() && !b.is_whitespace() => {
            !matches!(a, '[' | '(' | '/') && !matches!(b, ']' | ')' | ',' | '.' | ';' | ':')
        }
        _ => false,
    };
    if need_space {
        out.push(' ');
    }
}

fn first_display_char(component: &SemanticComponent) -> Option<char> {
    match component.kind {
        SemanticKind::Link | SemanticKind::Image => Some('['),
        _ => display_text(component)
            .chars()
            .next()
            .or_else(|| component.children.first().and_then(first_display_char)),
    }
}

fn format_link(component: &SemanticComponent) -> String {
    let label = display_text(component);
    let href = component.attrs.href.as_deref().unwrap_or("");
    format!("[{}]({})", escape_inline(label), escape_url(href))
}

fn format_image(component: &SemanticComponent) -> String {
    let alt = component
        .attrs
        .alt
        .as_deref()
        .or(component.label.as_deref())
        .or(component.text.as_deref())
        .unwrap_or("");
    let src = component.attrs.src.as_deref().unwrap_or("");
    format!("![{}]({})", escape_inline(alt), escape_url(src))
}

fn format_control(component: &SemanticComponent) -> String {
    match component.kind {
        SemanticKind::Input => {
            let input_type = component.attrs.input_type.as_deref().unwrap_or("text");
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            let label = component.label.as_deref().unwrap_or(name);
            let mut flags = Vec::new();
            if component.attrs.required == Some(true) {
                flags.push("required");
            }
            if component.attrs.disabled == Some(true) {
                flags.push("disabled");
            }
            if component.attrs.readonly == Some(true) {
                flags.push("readonly");
            }
            if component.attrs.checked == Some(true) {
                flags.push("checked");
            }
            let flag_s = if flags.is_empty() {
                String::new()
            } else {
                format!(" ({})", flags.join(", "))
            };
            let placeholder = component
                .attrs
                .placeholder
                .as_deref()
                .map(|p| format!(" placeholder=\"{}\"", escape_attr(p)))
                .unwrap_or_default();
            format!(
                "**Input** {} `{}` ({}): {}{}{}",
                escape_inline(label),
                escape_inline(name),
                escape_inline(input_type),
                escape_inline(value),
                placeholder,
                flag_s
            )
        }
        SemanticKind::Textarea => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            let label = component.label.as_deref().unwrap_or(name);
            format!(
                "**Textarea** {} `{}`: {}",
                escape_inline(label),
                escape_inline(name),
                escape_inline(value)
            )
        }
        SemanticKind::Select => {
            let name = component.attrs.name.as_deref().unwrap_or("");
            let value = component.attrs.value.as_deref().unwrap_or("");
            let label = component.label.as_deref().unwrap_or(name);
            let mut lines = vec![format!(
                "**Select** {} `{}`: {}",
                escape_inline(label),
                escape_inline(name),
                escape_inline(value)
            )];
            for option in &component.attrs.options {
                let mark = if option.selected { " *" } else { "" };
                let opt_label = option.label.as_deref().unwrap_or(option.value.as_str());
                lines.push(format!(
                    "  - {} ({}){}",
                    escape_inline(opt_label),
                    escape_inline(&option.value),
                    mark
                ));
            }
            lines.join("\n")
        }
        SemanticKind::Button => {
            let label = display_text(component);
            let button_type = component.attrs.button_type.as_deref().unwrap_or("button");
            let disabled = if component.attrs.disabled == Some(true) {
                " disabled"
            } else {
                ""
            };
            format!(
                "**Button** [{}] ({}){}",
                escape_inline(label),
                escape_inline(button_type),
                disabled
            )
        }
        _ => escape_inline(display_text(component)),
    }
}

fn push_ref_comment(out: &mut String, component: &SemanticComponent) {
    out.push_str(&format!(
        "<!-- semantic_ref=\"{}\" kind=\"{}\" -->\n",
        escape_attr(component.semantic_ref.as_str()),
        escape_attr(kind_name(component.kind)),
    ));
}

fn display_text(component: &SemanticComponent) -> &str {
    component
        .text
        .as_deref()
        .or(component.label.as_deref())
        .or(component.attrs.value.as_deref())
        .or(component.attrs.alt.as_deref())
        .unwrap_or("")
}

fn landmark_role_name(component: &SemanticComponent) -> String {
    component
        .attrs
        .landmark
        .map(|role| match role {
            crate::semantic::component::LandmarkRole::Main => "main",
            crate::semantic::component::LandmarkRole::Aside => "aside",
            crate::semantic::component::LandmarkRole::Header => "header",
            crate::semantic::component::LandmarkRole::Nav => "nav",
            crate::semantic::component::LandmarkRole::Section => "section",
            crate::semantic::component::LandmarkRole::Footer => "footer",
        })
        .unwrap_or("landmark")
        .to_string()
}

fn kind_name(kind: SemanticKind) -> &'static str {
    match kind {
        SemanticKind::Landmark => "landmark",
        SemanticKind::Heading => "heading",
        SemanticKind::Text => "text",
        SemanticKind::List => "list",
        SemanticKind::ListItem => "list_item",
        SemanticKind::Link => "link",
        SemanticKind::Image => "image",
        SemanticKind::Input => "input",
        SemanticKind::Textarea => "textarea",
        SemanticKind::Select => "select",
        SemanticKind::Button => "button",
        SemanticKind::Group => "group",
    }
}

fn escape_inline(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_url(value: &str) -> String {
    value
        .replace('\\', "%5C")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace(' ', "%20")
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn trim_trailing_blank(out: &mut String) {
    while out.ends_with("\n\n\n") {
        out.pop();
    }
    if out.ends_with("\n\n") {
        out.pop();
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
}
