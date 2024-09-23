use std::fmt::format;
use std::future::Future;
use base64::Engine;
use serde_json::{Map, Value};
use sqlx::{Error, FromRow, Pool, Sqlite};
use sqlx::types::JsonValue;
use tokio::runtime::{Handle, Runtime};
use crate::atlassian_document_utils::{get_mark_kind, indent_with, LinkAttrs, MarkKind, NodeLevel, StringWithNodeLevel, to_inline, to_top_level};

// specification of the atlassatian documentation format is available at
// https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/

fn json_map_to_html_string(json: &Map<String, Value>) -> String {
  let tmp = JsonValue::Object(json.clone()).to_string();
  let tmp_pretty = serde_json::from_str::<serde_json::Value>(&tmp);
  let tmp_pretty = tmp_pretty.and_then(|value: JsonValue| serde_json::to_string_pretty(&value));
  let text = match tmp_pretty {
    Ok(v) => v,
    Err(e) => { tmp }
  };

  let text = html_escape::encode_safe(text.as_str());
  let text = indent_with(text.as_ref(), "  ");

  let content = format!(
"<pre><code class=\"json_data\">
{text}
</code></pre><!-- json_data -->");
  content
}

fn string_to_sanitised_inline(input: &str) -> StringWithNodeLevel {
  let sanitised = html_escape::encode_safe(input);
  to_inline(sanitised.to_string())
}

fn string_to_sanitised_top_level(input: &str) -> StringWithNodeLevel {
  let content = html_escape::encode_safe(input);
  to_top_level(content.to_string())
}

fn json_to_toplevel_html_string(json: &Map<String, Value>) -> StringWithNodeLevel {
  let content = json_map_to_html_string(json);
  string_to_sanitised_top_level(content.as_str())
}

fn get_content_subobject_as_vec_html_string(
  json: &Map<String, Value>, db_conn: &Pool<Sqlite>
) -> Result<Vec<StringWithNodeLevel>, String> {
  let res = json
    .get("content")
    .and_then(|x| x.as_array())
    .and_then(|x| {
      let val = x
        .iter()
        .map(|x| value_to_html_string(x, db_conn))
        .collect::<Vec<_>>();

      Some(val)
    });

  let res = match res {
    None => { Err(json_map_to_html_string(json)) }
    Some(x) => { Ok(x) }
  };
  res
}

fn codeblock_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let inner_content = json
    .get("content")
    .and_then(|x| x.as_array())
    .and_then(|x| Some(array_of_value_to_html_string(x, db_conn)))
    .unwrap_or_else(|| json_to_toplevel_html_string(json));

  let language = json
    .get("attrs")
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("language"))
    .and_then(|x| x.as_str())
    .and_then(|x| Some(html_escape::encode_safe(x)))
    .unwrap_or_default();

  let inner_content = indent_with(inner_content.text.as_str(), "  ");
  let res = format!(
"<pre><code class=\"{language}\">
{inner_content}
</code></pre><!-- {language} -->"
);

  StringWithNodeLevel {
    text: res,
    node_level: NodeLevel::TopLevel,
  }
}

fn emoji_to_html_string(json: &Map<String, Value>) -> StringWithNodeLevel {
  let attrs = json
    .get("attrs")
    .and_then(|x| {
      if let Some(x) = x.get("text") {
        x.as_str()
      } else {
        x.get("shortName").and_then(|x| x.as_str())
      }
    })
    .and_then(|x| Some(html_escape::encode_safe(x)))
    .unwrap_or_default();

  let res = String::from(attrs);
  StringWithNodeLevel {
    text: res,
    node_level: NodeLevel::Inline,
  }
}

fn blockquote_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let inner_content = match json.get("content").and_then(|x| x.as_array()) {
    None => json_map_to_html_string(json),
    Some(content) => array_of_value_to_html_string(content, db_conn).text,
  };

  let inner_content = indent_with(inner_content.as_str(), "  ");

  let res = format!(
"<blockquote>
{inner_content}
</blockquote>");

  StringWithNodeLevel {
    text: res,
    node_level: NodeLevel::TopLevel,
  }
}

fn list_item_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let inner_content =
    get_content_subobject_as_vec_html_string(json, db_conn)
      .unwrap_or_else(|value| {
        //let content = string_to_sanitised_inline(value.as_str());
        let content = value; // when get_content_subobject_as_vec_html_string returns an error, it is a sanitised string
        // todo: implement new types for sanitised and unsanitised strings.
        vec![to_inline(content)]
      });

  let content = inner_content
    .into_iter()
    .map(|x| format!("<li>{x}</li>", x = x.text))
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_default();

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::ChildNode,
  }
}

fn bullet_list_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let inner_content = get_content_subobject_as_vec_html_string(json, db_conn);
  let inner_content = match inner_content {
    Ok(value) => value,
    Err(value) => {
      // when get_content_subobject_as_vec_html_string returns an error, it is a sanitised string
      // todo: implement new types for sanitised and unsanitised strings.
      return StringWithNodeLevel {
        text: value,
        node_level: NodeLevel::TopLevel,
      }
    }
  };

  let inner_content = inner_content
    .into_iter()
    .map(|s| { s.text })
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_default();

  let inner_content = indent_with(inner_content.as_str(), "  ");

  let content = format!(
"<ul>
{inner_content}
</ul>");

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::TopLevel,
  }
}

fn get_html_colour_from_mark(colour_kind: &Map<String, Value>) -> Result<String, String> {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/textColor/
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/backgroundColor/

  let Some(attrs) = colour_kind.get("attrs") else {
    return Err(String::from(
      "Error: colour mark does not have an attrs array",
    ));
  };

  let Some(attrs) = attrs.as_object() else {
    return Err(String::from(
      "Error: colour mark attrs attribute is not a json object.",
    ));
  };

  let Some(colour) = attrs.get("color") else {
    return Err(String::from(
      "Error: colour mark attrs attribute does not contain a href element",
    ));
  };

  let Some(colour) = colour.as_str() else {
    return Err(String::from("Error: colour mark element is not a string"));
  };

  if colour.len() != 7 {
    return Err(String::from(
      "Error: colour attribute is not an html hexadecimal colour (wrong length)",
    ));
  }

  let chars = colour.as_bytes();
  if chars[0] != ('#' as u8) {
    return Err(String::from(
      "Error: colour attribute is not an html hexadecimal colour (doesn't starts by #)",
    ));
  }

  for i in 1..=6 {
    if !chars[i].is_ascii_hexdigit() {
      return Err(String::from(
        "Error: colour attribute is not an html hexadecimal colour (not hexa value)",
      ));
    }
  }

  // todo: add support for dark mode
  let res = Ok(String::from(colour));
  res
}

fn get_background_html_colour_mark_kind(colour_kind: &Map<String, Value>) -> Result<MarkKind, String> {
  let res = get_html_colour_from_mark(colour_kind);
  match res {
    Ok(s) => Ok(MarkKind::BackgroundColour(s)),
    Err(e) => Err(e),
  }
}

fn get_text_html_colour_mark_kind(colour_kind: &Map<String, Value>) -> Result<MarkKind, String> {
  let res = get_html_colour_from_mark(colour_kind);
  match res {
    Ok(s) => Ok(MarkKind::TextColour(s)),
    Err(e) => Err(e),
  }
}

fn get_link_mark_kind(link_kind: &Map<String, Value>) -> Result<MarkKind, String> {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/marks/link/
  let Some(attrs) = link_kind.get("attrs") else {
    return Err(String::from(
      "Error: link mark does not have an attrs array",
    ));
  };

  let Some(attrs) = attrs.as_object() else {
    return Err(String::from(
      "Error: link mark attrs attribute is not a json object.",
    ));
  };

  let Some(href) = attrs.get("href") else {
    return Err(String::from(
      "Error: link mark attrs attribute does not contain a href element",
    ));
  };

  let Some(href) = href.as_str() else {
    return Err(String::from(
      "Error: link mark href element is not a string",
    ));
  };

  let collection = attrs.get("collection");
  let id = attrs.get("id");
  let occurrenceKey = attrs.get("occurrenceKey");
  let title = attrs.get("title");

  let to_option_string = |value: Option<&Value>| {
    value
      .and_then(|x| x.as_str())
      .and_then(|x| Some(html_escape::encode_safe(x)))
      .and_then(|x| Some(x.to_string()))
  };
  let collection = to_option_string(collection);
  let id = to_option_string(id);
  let occurrenceKey = to_option_string(occurrenceKey);
  let title = to_option_string(title);

  let href = html_escape::encode_safe(href);
  let href = href.to_string();

  let res = MarkKind::Link(LinkAttrs {
    collection,
    href,
    id,
    occurrenceKey,
    title,
  });

  Ok(res)
}

fn text_to_html_string(json: &Map<String, Value>) -> StringWithNodeLevel {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/text/
  let content = json
    .get("text")
    .and_then(|x| x.as_str())
    .and_then(|x| Some(html_escape::encode_safe(x)))
    .and_then(|x| Some(x.to_string()))
    .unwrap_or_default();

  let mut content = content;
  if let Some(marks) = json.get("marks") {
    if let Some(marks) = marks.as_array() {
      // https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/text/#marks

      // todo: sort attributes to ensure colours are at outmost level in the
      // generated content
      for mark in marks {
        content = match get_mark_kind(mark) {
          Ok(mark) => match mark {
            MarkKind::Code => {
              format!("<code>{content}</code>")
            }
            MarkKind::Emphasis => {
              format!("<em>{content}</em>")
            }
            MarkKind::Link(link_attrs) => {
              let title = match link_attrs.title {
                None => {String::from("")}
                Some(title) => {format!(" title=\"{title}\"")}
              };
              let url = link_attrs.href;
              format!("<a href=\"{url}\"{title}>{content}</a>")
            }
            MarkKind::Strike => {
              format!("<s>{content}</s>")
            }
            MarkKind::Strong => {
              format!("<strong>{content}</strong>")
            }
            MarkKind::Superscript => {
              format!("<sup>{content}</sup>")
            }
            MarkKind::SubScript => {
              format!("<sub>{content}</sub>")
            }
            MarkKind::TextColour(html_colour) => {
              format!("<span style=\"color:{html_colour}\">{content}</span>")
            }
            MarkKind::BackgroundColour(html_colour) => {
              format!("<span style=\"background-color:{html_colour}\">{content}</span>")
            },
            MarkKind::Underline => {
              format!("<span class=\"underline\">{content}</span>")
            }
          },
          Err(s) => {
            eprintln!("Error with mark: {s}");
            content
          }
        }
      }
    }
  }
  let content = content;

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::Inline,
  }
}

fn paragraph_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let inner_content = json
    .get("content")
    .and_then(serde_json::value::Value::as_array)
    .and_then(|x| Some(array_of_value_to_html_string(x, db_conn).text))
    .unwrap_or_default();

  let id = json
    .get("attrs")
    .and_then(serde_json::value::Value::as_object)
    .and_then(|x| x.get("localId"))
    .and_then(serde_json::value::Value::as_str)
    .and_then(|x| Some(html_escape::encode_safe(x)))
    .unwrap_or_default();

  let id_attr = if id.is_empty() {
    String::from("")
  } else {
    format!(" id=\"{id}\"")
  };

  let text = format!(
"<p{id_attr}>
{inner_content}
");

  StringWithNodeLevel {
    text,
    node_level: NodeLevel::TopLevel,
  }
}

fn doc_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let inner_content = json
    .get("content")
    .and_then(serde_json::value::Value::as_array)
    .and_then(|x| Some(array_of_value_to_html_string(x, db_conn).text))
    .unwrap_or_default();

  StringWithNodeLevel {
    text: inner_content,
    node_level: NodeLevel::TopLevel,
  }
}

fn hardbreak_to_html_string(_json: &Map<String, Value>) -> StringWithNodeLevel {
  StringWithNodeLevel {
    text: "<br/>\n".to_string(),
    node_level: NodeLevel::Inline,
  }
}

fn heading_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let inner_content = json
    .get("content")
    .and_then(|x| x.as_array())
    .and_then(|x| Some(array_of_value_to_html_string(x, db_conn).text))
    .unwrap_or_default();

  let level = json
    .get("attrs")
    .and_then(|x| x.get("level"))
    .and_then(|x| x.as_i64())
    .and_then(|x| Some(x.clamp(1, 6)))
    .unwrap_or_else(|| 1);

  let content = match level {
    1..=6 => format!("<h{level}>{inner_content}</h{level}>\n"),
    _ => {
      eprintln!("Error: heading levels should be between 1 and 6, got {level}");
      inner_content
    },
  };

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::TopLevel,
  }
}

fn mention_to_html_string(json: &Map<String, Value>) -> StringWithNodeLevel {
  let attrs = json
    .get("attrs")
    .and_then(|x| x.as_object());
  let Some(attrs) = attrs else {
    return StringWithNodeLevel {
      text: json_map_to_html_string(json),
      node_level: NodeLevel::Inline,
    };
  };

  let text = attrs.get("text")
    .and_then(|x| x.as_str())
    .and_then(|x| Some(html_escape::encode_safe(x)));

  if let Some(s) = text {
    return StringWithNodeLevel {
      text: String::from(s),
      node_level: NodeLevel::Inline,
    };
  }

  let id = attrs.get("id")
    .and_then(|x| x.as_str())
    .and_then(|x| Some(html_escape::encode_safe(x)));

  let content = match id {
    None => json_map_to_html_string(json),
    Some(s) => String::from(s),
  };

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::Inline,
  }
}

fn task_item_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let attrs = json.get("attrs").and_then(|x| x.as_object());
  let content = json.get("content").and_then(|x| x.as_array());

  let attrs = match attrs {
    None => { return json_to_toplevel_html_string(json) }
    Some(v) => {v}
  };

  let content = match content {
    None => { return json_to_toplevel_html_string(json) }
    Some(v) => {v}
  };

  let status = attrs
    .get("state")
    .and_then(|x| x.as_str())
    .unwrap_or_default();
  // todo: add ids and labels to checkboxes
  let checkbox = match status {
    "TODO" => "<input type=\"checkbox\" checked class=\"task_item_done\" />",
    "DONE" => "<input type=\"checkbox\" class=\"task_item_todo\" />",
    _ => "<input type=\"checkbox\" class=\"task_item_invalid\" />",
  };

  let content = array_of_value_to_html_string(content, db_conn);
  let content_string = format!("{checkbox} {content}", content = content.text);
  let content_string = indent_with(content_string.as_str(), "  ");
  let res_content = format!(
"<li class=\"task_item\">
{content_string}
</li>");

  StringWithNodeLevel {
    text: res_content,
    node_level: content.node_level,
  }
}

fn task_list_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json
    .get("content")
    .and_then(|x| x.as_array());

  let content = match content {
    None => { return json_to_toplevel_html_string(json) }
    Some(v) => {v}
  };

  let content = content
    .into_iter()
    .map(|x| value_to_html_string(x, db_conn))
    .collect::<Vec<_>>();

  let content = content
    .into_iter()
    .map(|x| x.text)
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_else(|| json_map_to_html_string(json));

  let content = indent_with(content.as_str(), "  ");
  let content = format!(
"<ul class=\"task_list\">
{content}
</ul>");

  let res = StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::TopLevel,
  };

  res
}

fn ordered_list_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json.get("content").and_then(|x| x.as_array());

  let Some(content) = content else {
    return StringWithNodeLevel {
      text: json_map_to_html_string(json),
      node_level: NodeLevel::ChildNode,
    };
  };

  let start_tag = json
    .get("attrs")
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("order"))
    .and_then(|x| x.as_u64())
    .and_then(|x| Some(format!(" start=\"{x}\"")))
    .unwrap_or_default();

  let content = content
    .into_iter()
    .map(|x| root_elt_doc_to_html_string(x, db_conn))
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_default();

  let content = format!(
"<ol{start_tag}>
{content}
</ol>");

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::ChildNode,
  }
}

fn panel_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let panel_type = json
    .get("attrs")
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("panelType"))
    .and_then(|x| x.as_str());

  let panel_type = match panel_type {
    Some(x)
    if (x == "info")
      || (x == "note")
      || (x == "warning")
      || (x == "success")
      || (x == "error") =>
      {
        x
      }
    _ => return json_to_toplevel_html_string(json),
  };

  let content = json
    .get("content")
    .and_then(|x| x.as_array())
    .and_then(|x| Some(array_of_value_to_html_string(x, db_conn).text))
    .unwrap_or_else(|| json_map_to_html_string(json));

  let content = indent_with(content.as_str(), "  ");

  let content = format!(
"<div class=\"{panel_type}\">
{content}
</div><!-- {panel_type} -->");

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::TopLevel,
  }
}

fn rule_to_html_string(_json: &Map<String, Value>) -> StringWithNodeLevel {
  StringWithNodeLevel {
    text: "<hr>".to_string(),
    node_level: NodeLevel::Inline,
  }
}

fn to_html_verbatim(val: &str) -> String {
  let val = html_escape::encode_safe(val);
  format!("<verbatim>{val}</verbatim>")
}

fn table_cell_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json
    .get("content")
    .and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    let res = to_top_level(content);
    return res;
  };

  let html_text = array_of_value_to_html_string(content, db_conn);
  let text = html_text.text;
  let attrs = get_style_str_for_table_cell_and_header(json);

  let res_text = format!("<td{attrs}>{text}</td>");
  StringWithNodeLevel {
    text: res_text,
    node_level: NodeLevel::TopLevel,
  }
}
fn table_row_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json.get("content").and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    return to_top_level(content);
  };

  let html_text = array_of_value_to_html_string(content, db_conn);

  let text = indent_with(html_text.text.as_str(), "  ");
  let res_text = format!(
"<tr>
{text}
</tr>");
  StringWithNodeLevel {
    text: res_text,
    node_level: NodeLevel::TopLevel,
  }
}

// on https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/table_cell/
// and equivalent page for table header, we can see that they take the same attributes.
// Seems to be a case of accidentally similar and therefore code shouldn't technically be
// factored in.
fn get_style_str_for_table_cell_and_header(json: &Map<String, Value>) -> String {
  let attrs = json
    .get("attrs")
    .and_then(|x| x.as_object());

  let background = attrs
    .and_then(|x| x.get("background"))
    .and_then(|x| x.as_str())
    .and_then(|x| Some(html_escape::encode_safe(x)))
    .and_then(|x| Some(format!(" style=\"background: {x};\"")))
    .unwrap_or_default();

  let colspan = attrs
    .and_then(|x| x.get("colspan"))
    .and_then(|x| x.as_u64())
    .and_then(|x| Some(format!(" colspan=\"{x}\"")))
    .unwrap_or_default();

  let rowspan = attrs
    .and_then(|x| x.get("rowspan"))
    .and_then(|x| x.as_u64())
    .and_then(|x| Some(format!(" rowspan=\"{x}\"")))
    .unwrap_or_default();

  // there is also a colwidth attribute, but doesn't easily map to an html/css attribute
  // and requires significantly more work to implement properly. Let's ignore that.

  let res = format!("{background}{colspan}{rowspan}");
  res
}

fn table_header_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json
    .get("content")
    .and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    return to_top_level(content);
  };

  let html_text = array_of_value_to_html_string(content, db_conn);
  let text = html_text.text;
  let attrs = get_style_str_for_table_cell_and_header(json);

  let res_text = format!("<th{attrs}>{text}</th>");
  StringWithNodeLevel {
    text: res_text,
    node_level: NodeLevel::TopLevel,
  }
}

fn table_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json
    .get("content")
    .and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    return to_top_level(content);
  };

  let attrs = json
    .get("attrs")
    .and_then(|x| x.as_object());

  let has_numbered_columns = attrs
    .and_then(|x| x.get("isNumberColumnEnabled"))
    .and_then(|x| x.as_bool())
    .unwrap_or(false);

  let width_style = attrs
    .and_then(|x| x.get("width"))
    .and_then(|x| x.as_u64())
    .and_then(|v| Some(format!("width: {v}px;")))
    .unwrap_or_default();

  let layout_style = attrs
    .and_then(|x| x.get("layout"))
    .and_then(|x| x.as_str())
    .and_then(|x| match x {
      "center" => Some("align-content: center;"),
      "align-start" => Some("align-content: flex-start;"),
      "default" | "wide" | "full_width" => {
        // according to https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/table/
        // those are deprecated and the width attribute should be used instead
        // Not relevant to us anyway
        None
      }
      _ => {
        eprintln!("Unknown layout style found");
        None
      }
    })
    .unwrap_or_default();

  // https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/table/
  // also defines a displayMode, but that one is not relevant for us

  let style_str = format!("{width_style}{layout_style}");
  let style_str = if style_str.is_empty() {
    style_str
  } else {
    format!(" style=\"{style_str}\"")
  };

  let mut cur_row = 0;
  let html_text = content
    .iter()
    .map(|x| {
      let v = value_to_html_string(x, db_conn).text;
      let v = if has_numbered_columns {
        if v.starts_with("<tr>\n  <td") {
          cur_row += 1;
          let replacement = format!("<tr>\n  <td>{cur_row}</td>");
          v.replace("<tr>", replacement.as_str())
        } else if v.starts_with("<tr>\n  <th") {
          v.replace("<tr>", "<tr>\n  <th></th>")
        } else {
          v
        }
      } else {
        v
      };
      v
    })
    .reduce(|a, b| format!("{a}\n{b}"));

  let html_text = match html_text {
    None => { return json_to_toplevel_html_string(json) }
    Some(v) => {v}
  };

  let html_text = indent_with(html_text.as_str(), "  ");
  let res_text = format!(
"<table{style_str}>
{html_text}
</table>");

  StringWithNodeLevel {
    text: res_text,
    node_level: NodeLevel::TopLevel,
  }
}

fn decision_list_to_html_string(decision_list: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  // decision list is not documented on https://developer.atlassian.com/cloud/jira/platform/apis/document/
  // This is taken from looking at the json generated by the ADF builder at
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/playground/
  // when creating a decision list

  let Some(content) = decision_list.get("content") else {
    return json_to_toplevel_html_string(decision_list);
  };

  let Some(content) = content.as_array() else {
    return json_to_toplevel_html_string(decision_list);
  };

  let content = content
    .iter()
    .map(|x| value_to_html_string(x, db_conn))
    .map(|a| format!("{a}", a = a.text))
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_default();

  let content = indent_with(content.as_str(), "  ");
  let content = format!(
"<ul class=\"decision-list\">
{content}
</ul>");

  StringWithNodeLevel {
    text: content,
    node_level: NodeLevel::TopLevel,
  }
}

fn decision_item_to_html_string(decision_item: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  // decision list is not documented on https://developer.atlassian.com/cloud/jira/platform/apis/document/
  // This is taken from looking at the json generated by the ADF builder at
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/playground/
  // when creating a decision list

  let content = decision_item
    .get("content")
    .and_then(|x| x.as_array());

  let content = match content {
    None => {return json_to_toplevel_html_string(decision_item);}
    Some(v) => {v}
  };

  let decision_state = decision_item
    .get("attrs")
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("state"))
    .and_then(|x| x.as_str())
    .unwrap_or_default();

  let decision_for_human = match decision_state {
    "DECIDED" => "decided",
    "UNDECIDED" => "undecided",
    _ => "unknown"
  };

  let decision_state = match decision_state {
    "DECIDED" => "decision-agreed-on",
    "UNDECIDED" => "decision-pending",
    _ => "decision-unknown"
  };

  // Looks like a decision can be either DECIDED or UNDECIDED
  // but not sure about other possibilities

  let res = array_of_value_to_html_string(content, db_conn);
  let res_text = indent_with(res.text.as_str(), "  ");
  let res_text = format!(
"<li class=\"{decision_state}\">
  <span class=\"decision_status\">decision: {decision_for_human}. </span>
{res_text}
</li>"
);

  let res = StringWithNodeLevel {
    text: res_text,
    node_level: res.node_level
  };
  res
}

fn get_file_data_from_uuid_in_db(media: &Map<String, Value>, db_conn: &Pool<Sqlite>, id: &str) -> Result<FileData, StringWithNodeLevel> {

  let query_str =
    "SELECT filename, file_size AS size, mime_type, content_data AS data
        FROM Attachment
        WHERE uuid = ?;";
  let query_res = tokio::task::block_in_place(move || {
    Handle::current().block_on(async move {
      sqlx::query_as::<_, FileData>(query_str)
        .bind(id)
        .fetch_one(&*db_conn)
        .await
    })
  });
  let query_res = match query_res {
    Ok(v) => { v }
    Err(e) => {
      eprintln!("Error: couldn't get data for file with uuid={id} from local database. Err; {e:?}");
      return Err(json_to_toplevel_html_string(media));
    }
  };
  Ok(query_res)
}

fn media_to_html_string(media: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {

  let attrs = media
    .get("attrs")
    .and_then(|x| x.as_object());

  let attrs = match attrs {
    None => {return json_to_toplevel_html_string(media)}
    Some(v) => {v}
  };

  let id = attrs
    .get("id")
    .and_then(|x| x.as_str());
  let id = match id {
    None => {return json_to_toplevel_html_string(media)}
    Some(v) => {v}
  };

  let id_type =  attrs
    .get("type")
    .and_then(|x| x.as_str());
  let id_type = match id_type {
    None => {return json_to_toplevel_html_string(media)}
    Some(v) => {v}
  };

  let width =  attrs
    .get("width")
    .and_then(|x| x.as_u64());

  let height =  attrs
    .get("height")
    .and_then(|x| x.as_u64());


  let text = match id_type {
    "file" => {
      let file_data = match get_file_data_from_uuid_in_db(media, db_conn, id) {
        Ok(value) => value,
        Err(value) => return value,
      };
      let base64_data = base64::engine::general_purpose::STANDARD.encode(file_data.data.as_slice());
      let mime_type = file_data.mime_type;
      let filename = file_data.filename;
      let width = match width {
        None => {String::from("")}
        Some(i) => { format!(" width=\"{i}\"") }
      };
      let height = match height {
        None => {String::from("")}
        Some(i) => { format!(" height=\"{i}\"") }
      };

      let text = match mime_type {
        mime_type if mime_type.starts_with("image/svg") => {
          // todo: validate that the svg image is valid svg
          String::from_utf8_lossy(file_data.data.as_slice()).to_string()
        }
        mime_type if mime_type.starts_with("image/") => {
          let mime_type = html_escape::encode_safe(mime_type.as_str());
          format!("<img{width}{height} src=\"data:{mime_type};base64,{base64_data}\">")
        }
        mime_type if mime_type.starts_with("video/") || mime_type.starts_with("audio/") => {
          let tag = mime_type.split('/').nth(0);
          let tag = match tag {
            None => { // could assert here since at this point, tag is either audio or video
              return json_to_toplevel_html_string(media)}
            Some(v) => {v}
          };
          let mime_type = html_escape::encode_safe(mime_type.as_str());
          let filename = html_escape::encode_safe(filename.as_str());
          let download_html_text = format!("<a href=\"data:{mime_type};base64,{base64_data}\" download=\"{filename}\">{filename}</a>");
          format!(
"<{tag}{width}{height} controls>
  <source src=\"data:{mime_type};base64,{base64_data}\" type=\"{mime_type}\">
  download {tag} file here: {download_html_text}
</{tag}>")
        }
        _ => {
          let filename = html_escape::encode_safe(filename.as_str());
          let mime_type = html_escape::encode_safe(mime_type.as_str());
          let download_html_text = format!("<a href=\"data:{mime_type};base64,{base64_data}\" download=\"{filename}\">{filename}</a>");
          download_html_text
        }
      };
      text
    }
    "link" => {
      let filename_attr = attrs
        .get("alt")
        .and_then(|x| x.as_str())
        .and_then(|x| Some(html_escape::encode_safe(x)))
        .and_then(|x| Some(format!(" filename=\"{x}\"")))
        .unwrap_or_default();

      let id = html_escape::encode_safe(id);
      let text = format!("<a href=\"{id}\"{filename_attr} download>{id}</a>");
      text
    }
    _ => {
      eprintln!("Invalid id type found: expecting [file] or [link] got [{id_type}]");
      return json_to_toplevel_html_string(media);
    }
  };

  let style_attr = match (width, height) {
    (None, None) => {String::from("")}
    _ => {
      let width_str = match width {
        None => {String::from("")}
        Some(i) => {format!("width: {i}px;")}
      };

      let height_str =  match height {
        None => {String::from("")}
        Some(i) => {format!("height: {i}px;")}
      };

      format!(" style=\"{height_str}{width_str}\"")
    }
  };

  let text = indent_with(text.as_str(), "  ");
  let text = format!(
"<div class=\"media\"{style_attr}>
{text}
</div>");

  let res = StringWithNodeLevel {
    text,
    node_level: NodeLevel::Inline,
  };
  res
}

fn media_single_to_html_string(media_single_item: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/mediaSingle/
  // says that media single has the following attributes:
  //
  // layout: determines the placement of the node on the page. wrap-left and wrap-right provide an image floated to the left or right of the page respectively, with text wrapped around it. center center aligns the image as a block, while wide does the same but bleeds into the margins. full-width makes the image stretch from edge to edge of the page.
  // width: determines the width of the image as a percentage of the width of the text content area. Has no effect if layout mode is wide or full-width.
  // widthType [optional] determines what the "unit" of the width attribute is presenting. Possible values are pixel and percentage. If the widthType attribute is undefined, it fallbacks to percentage.
  //
  // here, we simply ignore them

  let content = media_single_item
    .get("content")
    .and_then(|x| x.as_array())
    .and_then(|x| match x.as_slice() {
      [elt] => Some(elt), // media single contain an array of a single elt
      _ => None,
    })
    .and_then(|x| x.as_object());

  let content = match content {
    None => { return json_to_toplevel_html_string(media_single_item); }
    Some(v) => {v}
  };

  let is_media = content
    .get("type")
    .and_then(|x| x.as_str())
    .and_then(|x| Some(x == "media"))
    .unwrap_or(false);

  if !is_media {
    return json_to_toplevel_html_string(media_single_item);
  }

  // this is only a media element, ...
  media_to_html_string(content, db_conn)
}

#[derive(FromRow)]
struct FileData {
  filename: String,
  size: i64,
  mime_type: String,
  data: Vec<u8>
}

fn media_inline_to_html_string(media_inline_item: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  // on the web browser, jira UI displays media_inline_item as clickable links
  // inside the text. Clicking the link downloads the file.
  // Here, ... let's treat it like a media single item
  let attrs = media_inline_item
    .get("attrs")
    .and_then(|x| x.as_object());

  let attrs = match attrs {
    None => {return json_to_toplevel_html_string(media_inline_item)}
    Some(v) => {v}
  };

  let id = attrs
    .get("id")
    .and_then(|x| x.as_str());
  let id = match id {
    None => {return json_to_toplevel_html_string(media_inline_item)}
    Some(v) => {v}
  };

  let id_type =  attrs
    .get("type")
    .and_then(|x| x.as_str());
  let id_type = match id_type {
    None => {return json_to_toplevel_html_string(media_inline_item)}
    Some(v) => {v}
  };

  let text = match id_type {
    "file" => {
      let file_data = match get_file_data_from_uuid_in_db(media_inline_item, db_conn, id) {
        Ok(value) => value,
        Err(value) => return value,
      };
      let base64_data = base64::engine::general_purpose::STANDARD.encode(file_data.data);
      let mime_type = file_data.mime_type;
      let filename = file_data.filename;
      let mime_type = html_escape::encode_safe(mime_type.as_str());
      let filename = html_escape::encode_safe(filename.as_str());
      let text = format!("<a href=\"data:{mime_type};base64,{base64_data}\" download=\"{filename}\">{filename}</a>");
      text
    }
    "link" => {
      let filename_attr = attrs
        .get("alt")
        .and_then(|x| x.as_str())
        .and_then(|x| Some(html_escape::encode_safe(x)))
        .and_then(|x| Some(format!(" filename=\"{x}\"")))
        .unwrap_or_default();

      let id = html_escape::encode_safe(id);
      let text = format!("<a href=\"{id}\"{filename_attr} download>{id}</a>");
      text
    }
    _ => {
      eprintln!("Invalid id type found: expecting [file] or [link] got [{id_type}]");
      return json_to_toplevel_html_string(media_inline_item);
    }
  };

  let res = StringWithNodeLevel {
    text,
    node_level: NodeLevel::Inline,
  };
  res
}

fn inline_card_to_html_string(inline_card: &Map<String, Value>) -> StringWithNodeLevel {
  let Some(attrs) = inline_card.get("attrs") else {
    eprintln!("Invalid InlineCard found. Doesn't have an 'attrs' attribute");
    let res = json_map_to_html_string(inline_card);
    let res = to_inline(res);
    return res;
  };

  let Some(attrs) = attrs.as_object() else {
    eprintln!("Invalid InlineCard found. 'attrs' attribute isn't a json object");
    let res = json_map_to_html_string(inline_card);
    let res = to_inline(res);
    return res;
  };

  // https://developer.atlassian.com/cloud/jira/platform/apis/document/nodes/inlineCard/
  // says that either url or data must be provided, but not both
  let url = attrs.get("url");
  let data = attrs.get("data");

  let res = match (url, data) {
    (None, None) => {
      eprintln!("Invalid InlineCard found. 'attrs' doesn't contain an neither an 'url' not 'data' attribute");
      json_map_to_html_string(inline_card)
    },
    (Some(url), None) => {
      // the link above says that url must be a json object, but the provided
      // example displays url as a json string
      if let Some(url) = url.as_str() {
        let url = html_escape::encode_safe(url);
        format!("<a href=\"{url}\">{url}</a>")
      } else if let Some(url_as_object) = url.as_object() {
        json_map_to_html_string(url_as_object)
      } else {
        eprintln!("Invalid InlineCard found. 'url' is neither a string nor an object");
        json_map_to_html_string(inline_card)
      }
    },
    (Some(url), Some(data)) => {
      eprintln!("Invalid InlineCard found. 'attrs' contains both an 'url' and 'data' attributes. Only one expected");
      json_map_to_html_string(inline_card)
    },
    (None, Some(data)) => {
      match data.as_object() {
        None => {
          eprintln!("Invalid InlineCard found. 'attrs' contains a 'data' attributes, but it is not a json object");
          let data = format!("{data}");
          let data = html_escape::encode_safe(data.as_str());
          format!("<verbatim>{data}</verbatim>")
        },
        Some(data_as_object) => {
          json_map_to_html_string(data_as_object)
        }
      }
    }
  };

  StringWithNodeLevel {
    text: res,
    node_level: NodeLevel::Inline,
  }
}

fn media_group_to_html_string(media_group_item: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let Some(content) =  media_group_item.get("content") else {
    return json_to_toplevel_html_string(media_group_item);
  };

  let Some(content) = content.as_array() else {
    return json_to_toplevel_html_string(media_group_item);
  };

  let are_all_medias = content
    .iter()
    .all(|x| {
      let Some(x) = x.as_object() else {
        return false;
      };
      let Some(type_v) = x.get("type") else {
        return false;
      };
      let Some(type_v) = type_v.as_str() else {
        return false;
      };
      type_v == "media"
    });

  if !are_all_medias {
    return json_to_toplevel_html_string(media_group_item);
  }

  let res = array_of_value_to_html_string(content.as_ref(), db_conn);
  StringWithNodeLevel {
    text: res.text,
    node_level: NodeLevel::TopLevel,
  }
}

fn object_to_html_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let Some(type_elt) = json.get("type").and_then(|x| x.as_str()) else {
    return json_to_toplevel_html_string(json);
  };

  match type_elt {
    "blockquote" => blockquote_to_html_string(json, db_conn),
    "bulletList" => bullet_list_to_html_string(json, db_conn),
    "codeBlock" => codeblock_to_html_string(json, db_conn),
    "decisionList" => decision_list_to_html_string(json, db_conn),
    "decisionItem" => decision_item_to_html_string(json, db_conn),
    "doc" => doc_to_html_string(json, db_conn),
    "emoji" => emoji_to_html_string(json),
    "hardBreak" => hardbreak_to_html_string(json),
    "heading" => heading_to_html_string(json, db_conn),
    "inlineCard" => inline_card_to_html_string(json),
    "listItem" => list_item_to_html_string(json, db_conn),
    "media" => media_to_html_string(json, db_conn),
    "mediaInline" => media_inline_to_html_string(json, db_conn), // not in the documentation, but seen in the wild
    "mediaSingle" => media_single_to_html_string(json, db_conn),
    "mediaGroup" => media_group_to_html_string(json, db_conn),
    "mention" => mention_to_html_string(json),
    "orderedList" => ordered_list_to_html_string(json, db_conn),
    "panel" => panel_to_html_string(json, db_conn),
    "paragraph" => paragraph_to_html_string(json, db_conn),
    "rule" => rule_to_html_string(json),
    "table" => table_to_html_string(json, db_conn),
    "tableHeader" => table_header_to_html_string(json, db_conn),
    "tableCell" => table_cell_to_html_string(json, db_conn),
    "tableRow" => table_row_to_html_string(json, db_conn),
    "taskItem" => task_item_to_html_string(json, db_conn), // not in the documentation, but seen in the wild
    "taskList" => task_list_to_html_string(json, db_conn), // best is to try things in the playground https://developer.atlassian.com/cloud/jira/platform/apis/document/playground/
    "text" => text_to_html_string(json),
    _ => {
      eprintln!("Unknown type element '{type_elt}' in atlassian document format.");
      json_to_toplevel_html_string(json)
    }
  }
}

fn value_to_html_string(json: &JsonValue, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  match json {
    Value::Null => to_inline(String::from("null")),
    Value::Bool(n) => to_inline(n.to_string()), // String::from(n),
    Value::Number(n) => to_inline(n.to_string()), // String::from(n),
    Value::String(s) => string_to_sanitised_inline(s),
    Value::Array(n) => array_of_value_to_html_string(n, db_conn),
    Value::Object(o) => object_to_html_string(o, db_conn),
  }
}

fn merge_two_string_with_node_level(
  a: StringWithNodeLevel,
  b: StringWithNodeLevel,
) -> StringWithNodeLevel {

  let content = format!("{a}\n{b}", a = a.text, b = b.text);
  StringWithNodeLevel {
    text: content,
    node_level: b.node_level,
  }
}

fn array_of_value_to_html_string(content: &[JsonValue], db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let res = content
    .iter()
    .map(|x| value_to_html_string(x, db_conn))
    .reduce(merge_two_string_with_node_level);

  res.unwrap_or_else(|| to_inline(String::from("")))
}

pub(crate) fn root_elt_doc_to_html_string(description: &JsonValue, db_conn: &Pool<Sqlite>) -> String {
  let Some(val) = description.as_object() else {
    eprintln!("description is not a json object. It is {x}", x = description.to_string());
    return description.to_string();
  };

  let Some(type_val) = val.get("type") else {
    eprintln!("description is invalid. Must have a type key. It is {val:?}");
    return description.to_string();
  };

  let Some(type_val) = type_val.as_str() else {
    eprintln!("description is invalid. type key must be string It is {type_val:?}");
    return description.to_string();
  };

  if type_val.to_string() != "doc" {
    eprintln!("description is invalid. type key must be 'doc'. It is {type_val}");
    return description.to_string();
  }

  let Some(content) = val.get("content") else {
    eprintln!("val does not contain a element named 'content'. It is {val:?}");
    return description.to_string();
  };

  let Some(content) = content.as_array() else {
    eprintln!("val is not an array. It is {x}", x = content.to_string());
    return description.to_string();
  };

  let res = array_of_value_to_html_string(content, db_conn).text;
  res
}