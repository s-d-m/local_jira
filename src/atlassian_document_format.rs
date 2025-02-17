use crate::{atlassian_document_format, atlassian_document_utils};
use serde::de::Unexpected::Str;
use serde_json::{Map, Value};
use sqlx::types::JsonValue;
use std::fmt::format;
use toml::{to_string, to_string_pretty};
use atlassian_document_utils::emoji_to_string;
use crate::atlassian_document_utils::{get_background_colour_mark_kind, get_link_mark_kind, get_mark_kind, get_text_colour_mark_kind, indent_with, MarkKind, NodeLevel, StringWithNodeLevel, to_inline, to_top_level};

// specification of the atlassatian documentation format is available at
// https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/


fn json_map_to_string(json: &Map<String, Value>) -> String {
    let tmp = JsonValue::Object(json.clone()).to_string();
    let tmp_pretty = serde_json::from_str::<serde_json::Value>(&tmp);
    let tmp_pretty = tmp_pretty.and_then(|value: JsonValue| serde_json::to_string_pretty(&value));
    match tmp_pretty {
        Ok(v) => v,
        Err(e) => {
            return tmp;
        }
    }
}


fn json_to_toplevel_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let mut content = json_map_to_string(json);
    content.insert_str(0, "```json\n");
    content.push_str("\n```\n");

    let content = content;
    to_top_level(content)
}

fn get_content_subobject_as_vec_string(
    json: &Map<String, Value>,
) -> Result<Vec<StringWithNodeLevel>, String> {
    let res = json
        .get("content")
        .and_then(|x| x.as_array())
        .and_then(|x| Some(x.iter().map(value_to_string).collect::<Vec<_>>()))
        .and_then(|x| Some(Ok(x)))
        .unwrap_or_else(|| Err(json_map_to_string(json)));

    res
}

fn codeblock_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content = json
        .get("content")
        .and_then(|x| x.as_array())
        .and_then(|x| Some(array_of_value_to_string(x)))
        .unwrap_or_else(|| json_to_toplevel_string(json));

    let language = json
        .get("attrs")
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("language"))
        .and_then(|x| x.as_str())
        .unwrap_or_default();

    let inner_content = inner_content.text;
    let res = format!("```{language}\n{inner_content}\n```");
    StringWithNodeLevel {
        text: res,
        node_level: NodeLevel::TopLevel,
    }
}



fn blockquote_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content = match json.get("content").and_then(|x| x.as_array()) {
        None => json_map_to_string(json),
        Some(content) => array_of_value_to_string(content).text,
    };

    let res = indent_with(inner_content.as_str(), "> ");

    StringWithNodeLevel {
        text: res,
        node_level: NodeLevel::TopLevel,
    }
}

fn list_item_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content =
        get_content_subobject_as_vec_string(json).unwrap_or_else(|value| vec![to_top_level(value)]);

    let content = inner_content
        .into_iter()
        .map(|x| x.text)
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_default();

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::ChildNode,
    }
}

fn bullet_list_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content = match get_content_subobject_as_vec_string(json) {
        Ok(value) => value,
        Err(value) => {
            return StringWithNodeLevel {
                text: value,
                node_level: NodeLevel::TopLevel,
            }
        }
    };

    let content = inner_content
        .iter()
        .map(|s| {
            let bullet_item = s
                .text
                .lines()
                .map(|x| x.trim())
                .enumerate()
                .map(|(n, s)| match n {
                    0 => format!("  - {s}"),
                    _ => format!("    {s}"),
                })
                .reduce(|a, b| format!("{a}\n{b}"))
                .unwrap_or_default();
            bullet_item
        })
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_default();

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::TopLevel,
    }
}

fn text_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
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

            for mark in marks {
                content = match get_mark_kind(mark) {
                    Ok(mark) => match mark {
                        MarkKind::Code => {
                            format!("`{content}`")
                        }
                        MarkKind::Emphasis => {
                            format!("/{content}/")
                        }
                        MarkKind::Link(lind_attrs) => {
                            format!("[{content}]({url})", url = lind_attrs.href)
                        }
                        MarkKind::Strike => {
                            format!("~{content}~")
                        }
                        MarkKind::Strong => {
                            format!("*{content}*")
                        }
                        MarkKind::Superscript => {
                            format!("^{{{content}}}")
                        }
                        MarkKind::SubScript => {
                            format!("_{{{content}}}")
                        }
                        MarkKind::TextColour(_) | MarkKind::BackgroundColour(_) => content,
                        MarkKind::Underline => {
                            format!("_{content}_")
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

fn paragraph_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content = json
        .get("content")
        .and_then(serde_json::value::Value::as_array)
        .and_then(|x| Some(array_of_value_to_string(x).text))
        .unwrap_or_default();

    StringWithNodeLevel {
        text: inner_content,
        node_level: NodeLevel::TopLevel,
    }
}

fn doc_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content = json
        .get("content")
        .and_then(serde_json::value::Value::as_array)
        .and_then(|x| Some(array_of_value_to_string(x).text))
        .unwrap_or_default();

    StringWithNodeLevel {
        text: inner_content,
        node_level: NodeLevel::TopLevel,
    }
}

fn hardbreak_to_string(_json: &Map<String, Value>) -> StringWithNodeLevel {
    StringWithNodeLevel {
        text: "\n".to_string(),
        node_level: NodeLevel::Inline,
    }
}

fn heading_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content = json
        .get("content")
        .and_then(|x| x.as_array())
        .and_then(|x| Some(array_of_value_to_string(x).text))
        .unwrap_or_default();

    let level = json
        .get("attrs")
        .and_then(|x| x.get("level"))
        .and_then(|x| x.as_i64())
        .and_then(|x| Some(x.clamp(1, 6)))
        .unwrap_or_else(|| 1);

    let underline_with = |underline_char: char, inner_content: String| {
        inner_content
            .lines()
            .map(|x| {
                let len = x.len();
                let underline = underline_char.to_string().repeat(len);
                format!("{x}\n{underline}")
            })
            .reduce(|a, b| format!("{a}\n{b}"))
            .unwrap_or_default()
    };

    let to_level_1 = |inner_content: String| underline_with('=', inner_content);
    let to_level_2 = |inner_content: String| underline_with('-', inner_content);
    let to_level_n = |n: i64, inner_content: String| {
        let n: usize = n.try_into().unwrap_or(1);
        inner_content
            .lines()
            .map(|x| {
                let begin = String::from("#").repeat(n);
                format!("{begin} {x}")
            })
            .reduce(|a, b| format!("{a}\n{b}"))
            .unwrap_or_default()
    };

    let content = match level {
        1 => to_level_1(inner_content),
        2 => to_level_2(inner_content),
        3..=6 => to_level_n(level, inner_content),
        _ => {
            eprintln!("Error: heading levels should be between 1 and 6, got {level}");
            to_level_n(7, inner_content)
        },
    };

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::TopLevel,
    }
}

fn mention_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let attrs = json.get("attrs").and_then(|x| x.as_object());
    let Some(attrs) = attrs else {
        return StringWithNodeLevel {
            text: json_map_to_string(json),
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
        None => json_map_to_string(json),
        Some(s) => String::from(s),
    };

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::Inline,
    }
}

fn task_item_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let attrs = json
      .get("attrs")
      .and_then(|x| x.as_object());
    let content = json
      .get("content")
      .and_then(|x| x.as_array());

    if content.is_none() || attrs.is_none() {
        return json_to_toplevel_string(json);
    }

    let status = attrs
        .unwrap()
        .get("state")
        .and_then(|x| x.as_str())
        .unwrap_or_default();
    let beginning = match status {
        "TODO" => "☐",
        "DONE" => "☑",
        _ => "?",
    };

    let content_string = array_of_value_to_string(content.unwrap());
    let res_content = format!("{beginning} {x}", x = content_string.text);

    StringWithNodeLevel {
        text: res_content,
        node_level: content_string.node_level,
    }
}

fn task_list_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let content = json
      .get("content")
      .and_then(|x| x.as_array());

    let content = match content {
        None => { return json_to_toplevel_string(json) }
        Some(v) => {v}
    };

    let content = content
      .into_iter()
      .map(|x| value_to_string(x))
      .collect::<Vec<_>>();

    let content = content
      .into_iter()
      .map(|x| x.text)
      .reduce(|a, b| format!("{a}\n{b}"))
      .unwrap_or_else(|| json_map_to_string(json));

    let res = StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::TopLevel,
    };

    res
}
fn ordered_list_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let content = json.get("content").and_then(|x| x.as_array());

    let Some(content) = content else {
        return StringWithNodeLevel {
            text: json_map_to_string(json),
            node_level: NodeLevel::ChildNode,
        };
    };

    let init_num = json
        .get("attrs")
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("order"))
        .and_then(|x| x.as_u64())
        .unwrap_or(1);

    let content = content
        .into_iter()
        .map(|x| value_to_string(x))
        .map(|x| x.text)
        .collect::<Vec<_>>();

    let content = content
        .iter()
        .enumerate()
        .map(|(n, s)| format!("{pos}. {s}", pos = u64::try_from(n).unwrap_or(0) + init_num))
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_else(|| json_map_to_string(json));

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::ChildNode,
    }
}

fn panel_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
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
        _ => return json_to_toplevel_string(json),
    };

    let content = json
        .get("content")
        .and_then(|x| x.as_array())
        .and_then(|x| Some(array_of_value_to_string(x).text))
        .unwrap_or_else(|| json_map_to_string(json));

    let content = indent_with(&content, "| ");
    let padding_dash_len = panel_type.len();
    let padding_dash = "-".repeat(padding_dash_len + 2);
    let content = format!(
        "/---------- {panel_type} -----------\n{content}\n\\----------{padding_dash}-----------"
    );

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::TopLevel,
    }
}

fn rule_to_string(_json: &Map<String, Value>) -> StringWithNodeLevel {
    StringWithNodeLevel {
        text: "\n".to_string(),
        node_level: NodeLevel::Inline,
    }
}

fn to_html_verbatim(val: &str) -> String {
    format!("<verbatim>{val}</verbatim>")
}

fn table_cell_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let content = json.get("content").and_then(|x| x.as_array());

    let Some(content) = content else {
        let content = json_map_to_string(json);
        return to_top_level(content);
    };

    let html_text = array_of_value_to_string(content);
    // todo: support attrs

    let res_text = format!("<td>{text}</td>", text = html_text.text);
    StringWithNodeLevel {
        text: res_text,
        node_level: NodeLevel::TopLevel,
    }
}
fn table_row_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let content = json.get("content").and_then(|x| x.as_array());

    let Some(content) = content else {
        let content = json_map_to_string(json);
        return to_top_level(content);
    };

    let html_text = array_of_value_to_string(content);
    // todo: support attrs

    let res_text = format!("<tr>{text}</tr>", text = html_text.text);
    StringWithNodeLevel {
        text: res_text,
        node_level: NodeLevel::TopLevel,
    }
}

fn table_header_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let content = json.get("content").and_then(|x| x.as_array());

    let Some(content) = content else {
        let content = json_map_to_string(json);
        return to_top_level(content);
    };

    let html_text = array_of_value_to_string(content);
    // todo: support attrs

    let res_text = format!("<th>{text}</th>", text = html_text.text);
    StringWithNodeLevel {
        text: res_text,
        node_level: NodeLevel::TopLevel,
    }
}

fn table_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let content = json.get("content").and_then(|x| x.as_array());

    let Some(content) = content else {
        let content = json_map_to_string(json);
        return to_top_level(content);
    };

    let html_text = array_of_value_to_string(content);
    let res_text = format!("<table>{text}</table>", text = html_text.text);

    let res_text = html2text::from_read(res_text.as_bytes(), 80);

    StringWithNodeLevel {
        text: res_text,
        node_level: NodeLevel::TopLevel,
    }
}

fn decision_list_to_string(decision_list: &Map<String, Value>) -> StringWithNodeLevel {
    // decision list is not documented on https://developer.atlassian.com/cloud/jira/platform/apis/document/
    // This is taken from looking at the json generated by the ADF builder at
    // https://developer.atlassian.com/cloud/jira/platform/apis/document/playground/
    // when creating a decision list

    let Some(content) = decision_list.get("content") else {
        return json_to_toplevel_string(decision_list);
    };

    let Some(content) = content.as_array() else {
        return json_to_toplevel_string(decision_list);
    };

    let content = content
        .iter()
        .map(value_to_string)
        .map(|a| format!("{a}", a = a.text))
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_default();

    let res = format!("Decision list:\n{content}");

    StringWithNodeLevel {
        text: res,
        node_level: NodeLevel::TopLevel,
    }
}

fn decision_item_to_string(decision_item: &Map<String, Value>) -> StringWithNodeLevel {
    // decision list is not documented on https://developer.atlassian.com/cloud/jira/platform/apis/document/
    // This is taken from looking at the json generated by the ADF builder at
    // https://developer.atlassian.com/cloud/jira/platform/apis/document/playground/
    // when creating a decision list

    let Some(content) = decision_item.get("content") else {
        return json_to_toplevel_string(decision_item);
    };

    let Some(content) = content.as_array() else {
        return json_to_toplevel_string(decision_item);
    };

    let decision_state = decision_item
      .get("attrs")
      .and_then(|x| x.as_object())
      .and_then(|x| x.get("state"))
      .and_then(|x| x.as_str())
      .unwrap_or_default();

    let decision_state = match decision_state {
        "DECIDED" => "agreed on",
        "UNDECIDED" => "not yet agreed on",
        _ => "unknown"
    };

    // Looks like a decision can be either DECIDED or UNDECIDED
    // but not sure about other possibilities

    let res = array_of_value_to_string(content);
    let res_text = format!("Decision {decision_state}: {x}", x = res.text);
    StringWithNodeLevel {
        text: res_text,
        node_level: res.node_level
    }
}

fn media_to_string(media: &Map<String, Value>) -> StringWithNodeLevel {
    let res_str = json_map_to_string(media);
    let res_str = format!("```json
{res_str}
```");

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
        return json_to_toplevel_string(media_single_item);
    };

    let Some(content) = content.as_array() else {
        return json_to_toplevel_string(media_single_item);
    };

    let content = match &content[..] {
        [elt] => elt,
        _ => {return json_to_toplevel_string(media_single_item);}
    };

    let Some(value) = content.as_object() else {
        return json_to_toplevel_string(media_single_item);
    };

    let Some(value_type) = value.get("type") else {
        return json_to_toplevel_string(media_single_item);
    };

    let Some(value_type) = value_type.as_str() else {
        return json_to_toplevel_string(media_single_item);
    };

    let media = match value_type {
        "media" => value,
        _ => return json_to_toplevel_string(media_single_item),
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
        let res = json_map_to_string(inline_card);
        let res = to_inline(res);
        return res;
    };

    let Some(attrs) = attrs.as_object() else {
        eprintln!("Invalid InlineCard found. 'attrs' attribute isn't a json object");
        let res = json_map_to_string(inline_card);
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
            json_map_to_string(inline_card)
        },
        (Some(url), None) => {
            // the link above says that url must be a json object, but the provided
            // example displays url as a json string
            if let Some(url_as_str) = url.as_str() {
                 url_as_str.to_string()
            } else if let Some(url_as_object) = url.as_object() {
                json_map_to_string(url_as_object)
            } else {
                eprintln!("Invalid InlineCard found. 'url' is neither a string nor an object");
                url.to_string()
            }
        },
        (Some(url), Some(data)) => {
            eprintln!("Invalid InlineCard found. 'attrs' contains both an 'url' and 'data' attributes. Only one expected");
            json_map_to_string(inline_card)
        },
        (None, Some(data)) => {
            match data.as_object() {
                None => {
                    eprintln!("Invalid InlineCard found. 'attrs' contains a 'data' attributes, but it is not a json object");
                    data.to_string()
                },
                Some(data_as_object) => {
                    json_map_to_string(data_as_object)
                }
            }
        }
    };

    StringWithNodeLevel {
        text: res,
        node_level: NodeLevel::Inline,
    }
}

fn media_group_to_string(media_group_item: &Map<String, Value>) -> StringWithNodeLevel {
    let Some(content) =  media_group_item.get("content") else {
        return json_to_toplevel_string(media_group_item);
    };

    let Some(content) = content.as_array() else {
        return json_to_toplevel_string(media_group_item);
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
        return json_to_toplevel_string(media_group_item);
    }
    
    let res = array_of_value_to_string(content.as_ref());
    StringWithNodeLevel {
        text: res.text,
        node_level: NodeLevel::TopLevel,
    }
}

fn object_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let Some(type_elt) = json.get("type").and_then(|x| x.as_str()) else {
        return json_to_toplevel_string(json);
    };

    match type_elt {
        "blockquote" => blockquote_to_string(json),
        "bulletList" => bullet_list_to_string(json),
        "codeBlock" => codeblock_to_string(json),
        "decisionList" => decision_list_to_string(json),
        "decisionItem" => decision_item_to_string(json),
        "doc" => doc_to_string(json),
        "emoji" => emoji_to_string(json),
        "hardBreak" => hardbreak_to_string(json),
        "heading" => heading_to_string(json),
        "inlineCard" => inline_card_to_string(json),
        "listItem" => list_item_to_string(json),
        "media" => media_to_string(json),
        "mediaInline" => media_inline_to_string(json), // not in the documentation, but seen in the wild
        "mediaSingle" => media_single_to_string(json),
        "mediaGroup" => media_group_to_string(json),
        "mention" => mention_to_string(json),
        "orderedList" => ordered_list_to_string(json),
        "panel" => panel_to_string(json),
        "paragraph" => paragraph_to_string(json),
        "rule" => rule_to_string(json),
        "table" => table_to_string(json),
        "tableHeader" => table_header_to_string(json),
        "tableCell" => table_cell_to_string(json),
        "tableRow" => table_row_to_string(json),
        "taskItem" => task_item_to_string(json), // not in the documentation, but seen in the wild
        "taskList" => task_list_to_string(json), // best is to try things in the playground https://developer.atlassian.com/cloud/jira/platform/apis/document/playground/
        "text" => text_to_string(json),
        _ => {
            eprintln!("Unknown type element '{type_elt}' in atlassian document format.");
            json_to_toplevel_string(json)
        }
    }
}

fn value_to_string(json: &JsonValue) -> StringWithNodeLevel {
    match json {
        Value::Null => to_inline(String::from("null")),
        Value::Bool(n) => to_inline(n.to_string()), // String::from(n),
        Value::Number(n) => to_inline(n.to_string()), // String::from(n),
        Value::String(n) => to_inline(String::from(n)),
        Value::Array(n) => array_of_value_to_string(n),
        Value::Object(o) => object_to_string(o),
    }
}

fn merge_two_string_with_node_level(
    a: StringWithNodeLevel,
    b: StringWithNodeLevel,
) -> StringWithNodeLevel {
    let separator = match (a.node_level, b.node_level) {
        (NodeLevel::TopLevel, NodeLevel::TopLevel) => "\n\n",
        (NodeLevel::TopLevel, NodeLevel::ChildNode) => "\n",
        (NodeLevel::TopLevel, NodeLevel::Inline) => "\n",

        (NodeLevel::ChildNode, NodeLevel::TopLevel) => "\n",
        (NodeLevel::ChildNode, NodeLevel::ChildNode) => "\n",
        (NodeLevel::ChildNode, NodeLevel::Inline) => "",

        (NodeLevel::Inline, NodeLevel::TopLevel) => "\n",
        (NodeLevel::Inline, NodeLevel::ChildNode) => "\n",
        (NodeLevel::Inline, NodeLevel::Inline) => "",
    };

    let content = format!("{a}{separator}{b}", a = a.text, b = b.text);
    StringWithNodeLevel {
        text: content,
        node_level: b.node_level,
    }
}

fn array_of_value_to_string(content: &[JsonValue]) -> StringWithNodeLevel {
    let res = content
        .iter()
        .map(value_to_string)
        .reduce(merge_two_string_with_node_level);

    res.unwrap_or_else(|| to_inline(String::from("")))
}

pub(crate) fn root_elt_doc_to_string(description: &JsonValue) -> String {
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

    let res = array_of_value_to_string(content).text;
    res
}
