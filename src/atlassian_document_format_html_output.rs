use std::fmt::format;
use serde_json::{Map, Value};
use sqlx::{Pool, Sqlite};
use sqlx::types::JsonValue;
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

  let text = indent_with(text.as_str(), "  ");

  let content = format!(
"<pre><code class=\"json_code\">
{text}
</code></pre><!-- json_code -->");
  content
}

fn json_to_toplevel_html_string(json: &Map<String, Value>) -> StringWithNodeLevel {
  let content = json_map_to_html_string(json);
  to_top_level(content)
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
    })
    .and_then(|x| Some(Ok(x)))
    .unwrap_or_else(|| Err(json_map_to_html_string(json)));

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
      .unwrap_or_else(|value| vec![to_top_level(value)]);

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
      .and_then(|x| Some(x.to_string()))
  };
  let collection = to_option_string(collection);
  let id = to_option_string(id);
  let occurrenceKey = to_option_string(occurrenceKey);
  let title = to_option_string(title);

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
              format!("<a href=\"{url}\"{title}>{content}</a>", url = link_attrs.href)
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
    .and_then(|x| x.as_str());

  if let Some(s) = text {
    return StringWithNodeLevel {
      text: String::from(s),
      node_level: NodeLevel::Inline,
    };
  }

  let id = attrs.get("id")
    .and_then(|x| x.as_str());

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
  format!("<verbatim>{val}</verbatim>")
}

fn table_cell_to_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json
    .get("content")
    .and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    let res = to_top_level(content);
    return res;
  };

  let html_text = array_of_value_to_html_string(content, db_conn);
  // todo: support attrs

  let res_text = format!("<td>{text}</td>", text = html_text.text);
  StringWithNodeLevel {
    text: res_text,
    node_level: NodeLevel::TopLevel,
  }
}
fn table_row_to_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json.get("content").and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    return to_top_level(content);
  };

  let html_text = array_of_value_to_html_string(content, db_conn);
  // todo: support attrs

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

fn table_header_to_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json.get("content").and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    return to_top_level(content);
  };

  let html_text = array_of_value_to_html_string(content, db_conn);
  // todo: support attrs

  let res_text = format!("<th>{text}</th>", text = html_text.text);
  StringWithNodeLevel {
    text: res_text,
    node_level: NodeLevel::TopLevel,
  }
}

fn table_to_string(json: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  let content = json.get("content").and_then(|x| x.as_array());

  let Some(content) = content else {
    let content = json_map_to_html_string(json);
    return to_top_level(content);
  };

  let html_text = array_of_value_to_html_string(content, db_conn);
  let res_text = indent_with(html_text.text.as_str(), "  ");
  let res_text = format!(
"<table>
{res_text}
</table>");

  StringWithNodeLevel {
    text: res_text,
    node_level: NodeLevel::TopLevel,
  }
}

fn decision_list_to_string(decision_list: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
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
    .map(|a| format!("  decision: {}", a.text))
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_default();

  let res = format!("Decision list:\n{content}");

  StringWithNodeLevel {
    text: res,
    node_level: NodeLevel::TopLevel,
  }
}

fn decision_item_to_string(decision_item: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
  // decision list is not documented on https://developer.atlassian.com/cloud/jira/platform/apis/document/
  // This is taken from looking at the json generated by the ADF builder at
  // https://developer.atlassian.com/cloud/jira/platform/apis/document/playground/
  // when creating a decision list

  let Some(content) = decision_item.get("content") else {
    return json_to_toplevel_html_string(decision_item);
  };

  let Some(content) = content.as_array() else {
    return json_to_toplevel_html_string(decision_item);
  };

  let res = array_of_value_to_html_string(content, db_conn);
  res
}

fn media_to_string(media: &Map<String, Value>) -> StringWithNodeLevel {
  let res_str = json_map_to_html_string(media);

  // the media node doesn't really fit for a text output.
  // could try to do interesting things like displaying images in the terminal,
  // create clickable links for terminals supporting them etc
  // instead, just dump the json here.

  StringWithNodeLevel {
    text: res_str,
    node_level: NodeLevel::ChildNode,
  }
}

fn media_single_to_string(media_single_item: &Map<String, Value>) -> StringWithNodeLevel {
  let Some(content) = media_single_item.get("content") else {
    return json_to_toplevel_html_string(media_single_item);
  };

  let Some(content) = content.as_array() else {
    return json_to_toplevel_html_string(media_single_item);
  };

  let content = match &content[..] {
    [elt] => elt,
    _ => {return json_to_toplevel_html_string(media_single_item);}
  };

  let Some(value) = content.as_object() else {
    return json_to_toplevel_html_string(media_single_item);
  };

  let Some(value_type) = value.get("type") else {
    return json_to_toplevel_html_string(media_single_item);
  };

  let Some(value_type) = value_type.as_str() else {
    return json_to_toplevel_html_string(media_single_item);
  };

  let media = match value_type {
    "media" => value,
    _ => return json_to_toplevel_html_string(media_single_item),
  };

  // mediaSingle contains a single media element, and have the following attributes:
  // - layout (wrap-left / center / ... / wide / ...)
  // - width (optional)
  // - widthType (pixels or percentage)
  // These attributes do not hold for a simple text format. Hence let's
  // ignore them and treat the mediaSingle node, like a media node.

  let res = media_to_string(media);
  StringWithNodeLevel {
    text: res.text,
    node_level: NodeLevel::TopLevel,
  }
}

fn media_inline_to_string(media_inline_item: &Map<String, Value>) -> StringWithNodeLevel {
  // on the web browser, jira UI displays media_inline_item as clickable links
  // inside the text. Clicking the link downloads the file.
  // Here, ... let's treat it like a media single item
  media_single_to_string(media_inline_item)
}

fn inline_card_to_string(inline_card: &Map<String, Value>) -> StringWithNodeLevel {
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
      if let Some(url_as_str) = url.as_str() {
        url_as_str.to_string()
      } else if let Some(url_as_object) = url.as_object() {
        json_map_to_html_string(url_as_object)
      } else {
        eprintln!("Invalid InlineCard found. 'url' is neither a string nor an object");
        url.to_string()
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
          data.to_string()
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

fn media_group_to_string(media_group_item: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> StringWithNodeLevel {
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
    "decisionList" => decision_list_to_string(json, db_conn),
    "decisionItem" => decision_item_to_string(json, db_conn),
    "doc" => doc_to_html_string(json, db_conn),
    "emoji" => emoji_to_html_string(json),
    "hardBreak" => hardbreak_to_html_string(json),
    "heading" => heading_to_html_string(json, db_conn),
    "inlineCard" => inline_card_to_string(json),
    "listItem" => list_item_to_html_string(json, db_conn),
    "media" => media_to_string(json),
    "mediaInline" => media_inline_to_string(json), // not in the documentation, but seen in the wild
    "mediaSingle" => media_single_to_string(json),
    "mediaGroup" => media_group_to_string(json, db_conn),
    "mention" => mention_to_html_string(json),
    "orderedList" => ordered_list_to_html_string(json, db_conn),
    "panel" => panel_to_html_string(json, db_conn),
    "paragraph" => paragraph_to_html_string(json, db_conn),
    "rule" => rule_to_html_string(json),
    "table" => table_to_string(json, db_conn),
    "tableHeader" => table_header_to_string(json, db_conn),
    "tableCell" => table_cell_to_string(json, db_conn),
    "tableRow" => table_row_to_string(json, db_conn),
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
    Value::String(n) => to_inline(String::from(n)),
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