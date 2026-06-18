use leptonic::prelude::TiptapContent;

pub(crate) fn rich_text_editor_html(value: &str) -> String {
    if looks_like_rich_text_html(value) {
        value.to_owned()
    } else {
        plain_text_to_editor_html(value)
    }
}

fn looks_like_rich_text_html(value: &str) -> bool {
    let value = value.trim_start().to_ascii_lowercase();
    [
        "<blockquote",
        "<br",
        "<div",
        "<h1",
        "<h2",
        "<h3",
        "<h4",
        "<h5",
        "<h6",
        "<ol",
        "<p",
        "<pre",
        "<ul",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn plain_text_to_editor_html(value: &str) -> String {
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    if value.is_empty() {
        return String::new();
    }

    value
        .split("\n\n")
        .map(|paragraph| {
            let lines = paragraph
                .lines()
                .map(escape_html_text)
                .collect::<Vec<_>>()
                .join("<br>");
            if lines.is_empty() {
                "<p><br></p>".to_owned()
            } else {
                format!("<p>{lines}</p>")
            }
        })
        .collect::<String>()
}

fn escape_html_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

pub(crate) fn rich_text_plain_text(value: &str) -> String {
    if !looks_like_rich_text_html(value) {
        return value.to_owned();
    }

    let mut text = String::with_capacity(value.len());
    let mut tag = String::new();
    let mut inside_tag = false;
    for character in value.chars() {
        if inside_tag {
            if character == '>' {
                append_text_boundary_for_html_tag(&tag, &mut text);
                tag.clear();
                inside_tag = false;
            } else {
                tag.push(character);
            }
        } else if character == '<' {
            inside_tag = true;
        } else {
            text.push(character);
        }
    }

    decode_basic_html_entities(text.trim())
}

fn append_text_boundary_for_html_tag(tag: &str, text: &mut String) {
    let tag = tag.trim().trim_start_matches('/');
    let Some(name) = tag.split_whitespace().next() else {
        return;
    };
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "blockquote" | "br" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "p" | "pre"
    ) && !text.ends_with('\n')
        && !text.is_empty()
    {
        text.push('\n');
    }
}

fn decode_basic_html_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

pub(crate) fn tiptap_content_to_string(content: TiptapContent) -> String {
    match content {
        TiptapContent::Html(content) => content,
        TiptapContent::Json(content) => content.to_string(),
    }
}

pub(crate) fn normalize_tiptap_storage_value(value: String) -> String {
    if rich_text_plain_text(&value).trim().is_empty() {
        String::new()
    } else {
        value
    }
}
