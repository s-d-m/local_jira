use serde::de::Unexpected::Str;
use crate::atlassian_document_format;
use serde_json::{Map, Value};
use sqlx::types::JsonValue;

// specification of the atlassatian documentation format is available at
// https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/

#[derive(Copy, Clone, Debug)]
enum NodeLevel {
    TopLevel,
    ChildNode,
    Inline,
}

#[derive(Debug)]
struct StringWithNodeLevel {
    text: String,
    node_level: NodeLevel,
}

fn indent_with(text: &str, lines_starter: &str) -> String {
    text.lines()
        .map(|x| format!("{lines_starter}{x}"))
        .reduce(|a, b| format!("{a}\n{b}"))
        .unwrap_or_default()
}

fn json_map_to_string(json: &Map<String, Value>) -> String {
    JsonValue::Object(json.clone()).to_string()
}

fn to_inline(content: String) -> StringWithNodeLevel {
    StringWithNodeLevel { text: content, node_level: NodeLevel::Inline }
}

fn to_top_level(content: String) -> StringWithNodeLevel {
    StringWithNodeLevel { text: content, node_level: NodeLevel::TopLevel }
}


fn json_to_toplevel_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let content = json_map_to_string(json);
    to_top_level(content)
}

fn get_content_subobject_as_vec_string(json: &Map<String, Value>) -> Result<Vec<StringWithNodeLevel>, String> {
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

fn emoji_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
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
            let bullet_item = s.text
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
    if json.get("marks").is_some() {
        eprintln!("WARNING, marks are ignored for now");
    };

    let content = json
        .get("text")
        .and_then(|x| x.as_str())
        .and_then(|x| Some(x.to_string()))
        .unwrap_or_default();

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::Inline,
    }
}

fn paragraph_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let inner_content = json
        .get("content")
        .and_then(|x| x.as_array())
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
        .and_then(|x| x.as_array())
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
        _ => panic!("heading levels should be between 1 and 6, got {level}"),
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

    let text = attrs.get("text").and_then(|x| x.as_str());

    if let Some(s) = text {
        return StringWithNodeLevel {
            text: String::from(s),
            node_level: NodeLevel::Inline,
        };
    }

    let id = attrs.get("id").and_then(|x| x.as_str());

    let content = match id {
        None => json_map_to_string(json),
        Some(s) => String::from(s),
    };

    StringWithNodeLevel {
        text: content,
        node_level: NodeLevel::Inline,
    }
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
        .map(|x| root_elt_doc_to_string(x))
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
    let content =
    format!("/---------- {panel_type} -----------\n{content}\n\\----------{padding_dash}-----------");

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

fn object_to_string(json: &Map<String, Value>) -> StringWithNodeLevel {
    let Some(type_elt) = json.get("type").and_then(|x| x.as_str()) else {
        return json_to_toplevel_string(json);
    };

    match type_elt {
        "blockquote" => blockquote_to_string(json),
        "bulletList" => bullet_list_to_string(json),
        "codeBlock" => codeblock_to_string(json),
        "doc" => doc_to_string(json),
        "emoji" => emoji_to_string(json),
        "hardBreak" => hardbreak_to_string(json),
        "heading" => heading_to_string(json),
        // "inlineCard" => inlinecard_to_string(json),
        "listItem" => list_item_to_string(json),
        // media => todo!(),
        "mention" => mention_to_string(json),
        "orderedList" => ordered_list_to_string(json),
        "panel" => panel_to_string(json),
        "paragraph" => paragraph_to_string(json),
        "rule" => rule_to_string(json),
        // table
        "text" => text_to_string(json),
        _ => json_to_toplevel_string(json),
    }
}

fn value_to_string(json: &JsonValue) -> StringWithNodeLevel {
    match json {
        Value::Null => to_inline(String::from("null")),
        Value::Bool(n) => to_inline(n.to_string()),   // String::from(n),
        Value::Number(n) => to_inline(n.to_string()), // String::from(n),
        Value::String(n) => to_inline(String::from(n)),
        Value::Array(n) => array_of_value_to_string(n),
        Value::Object(o) => object_to_string(o),
    }
}

fn merge_two_string_with_node_level(a: StringWithNodeLevel, b: StringWithNodeLevel) -> StringWithNodeLevel {
    let res_level = b.node_level;

    let separator = match (a.node_level, b.node_level) {
        (NodeLevel::TopLevel, NodeLevel::TopLevel) => { "\n\n" }
        (NodeLevel::TopLevel, NodeLevel::ChildNode) => { "\n"}
        (NodeLevel::TopLevel, NodeLevel::Inline) => { "\n"}

        (NodeLevel::ChildNode, NodeLevel::TopLevel) => { "\n"}
        (NodeLevel::ChildNode, NodeLevel::ChildNode) => {"\n"}
        (NodeLevel::ChildNode, NodeLevel::Inline) => {""}

        (NodeLevel::Inline, NodeLevel::TopLevel) => {"\n"}
        (NodeLevel::Inline, NodeLevel::ChildNode) => {"\n"}
        (NodeLevel::Inline, NodeLevel::Inline) => {""}
    };

    let content = format!("{a}{separator}{b}", a = a.text, b = b.text);
    StringWithNodeLevel {
        text: content,
        node_level: b.node_level,
    }
}

fn array_of_value_to_string(content: &Vec<JsonValue>) -> StringWithNodeLevel {
    let res = content
        .iter()
        .map(|x| value_to_string(x))
        .reduce(|a, b| merge_two_string_with_node_level(a, b));


    res.unwrap_or_else(|| to_inline(String::from("")))
}

pub(crate) fn root_elt_doc_to_string(description: &JsonValue) -> String {
    let res = description
        .as_object()
        .and_then(|x| x.get("content"))
        .and_then(|x| x.as_array())
        .and_then(|x| Some(array_of_value_to_string(x).text))
        .unwrap_or_else(|| description.to_string());

    res
}
