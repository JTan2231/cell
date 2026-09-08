//! Mentor's message routing and text handling. Email owns the transport.

use crate::store::{Assignment, Incoming};
use crate::{Result, fail};
use email::api::{Message, ReceivedMessage, ReplyOptions};
use scraper::{Html, Node};
use serde::{Deserialize, Serialize};

const MAX_ANSWER_BYTES: usize = 128 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenEmail {
    pub message: Message,
    pub reply: ReplyOptions,
}

pub fn problem_email(assignment: &Assignment, title: &str, prompt: &str) -> FrozenEmail {
    FrozenEmail {
        message: Message {
            subject: title.into(),
            body: prompt.into(),
            idempotency_key: Some(format!("mentor/problem/{}",assignment.date)),
        },
        reply: ReplyOptions { reply_to:Some(assignment.reply_to.clone()), ..ReplyOptions::default() },
    }
}

pub fn response_email(assignment: &Assignment, incoming: &Incoming, title: &str, body: &str) -> FrozenEmail {
    FrozenEmail {
        message: Message {
            subject:format!("Re: {title}"),
            body:body.into(),
            idempotency_key:Some(format!("mentor/critique/{}",incoming.id)),
        },
        reply:ReplyOptions {
            reply_to:Some(assignment.reply_to.clone()),
            in_reply_to:Some(incoming.message_id.clone()),
            references:incoming.references.clone(),
        },
    }
}

pub fn mailbox(value: &str) -> Option<String> {
    if value.len() > 512 || value.chars().any(char::is_control) { return None; }
    let address = if let Some((_,rest)) = value.rsplit_once('<') {
        rest.strip_suffix('>')?.trim()
    } else { value.trim() };
    let (local,domain) = address.rsplit_once('@')?;
    if local.is_empty() || domain.is_empty() || address.chars().any(|c| c.is_whitespace() || matches!(c,'<'|'>'|','|';')) { return None; }
    Some(address.to_ascii_lowercase())
}

pub fn assignment_token(address: &str) -> Option<String> {
    let address = mailbox(address)?;
    let (local,_) = address.rsplit_once('@')?;
    let token = local.strip_prefix("mentor.")?;
    if token.len() != 32 || !token.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return None; }
    Some(token.into())
}

pub fn header<'a>(message: &'a ReceivedMessage, name: &str) -> Option<&'a str> {
    message.headers.iter().find(|(key,_)|key.eq_ignore_ascii_case(name)).map(|(_,value)|value.as_str())
}

pub fn automated(message: &ReceivedMessage) -> bool {
    header(message,"auto-submitted").is_some_and(|v|!v.trim().eq_ignore_ascii_case("no"))
        || header(message,"precedence").is_some_and(|v|matches!(v.trim().to_ascii_lowercase().as_str(),"bulk"|"junk"|"list"))
        || header(message,"list-id").is_some()
}

pub fn valid_message_id(value: &str) -> bool {
    value.len() >= 5 && value.len() <= 512 && value.starts_with('<') && value.ends_with('>')
        && value[1..value.len()-1].split_once('@').is_some_and(|(left,right)|!left.is_empty() && !right.is_empty() && !right.contains('@'))
        && value[1..value.len()-1].bytes().all(|b|b.is_ascii_graphic() && !b"<>\\\"".contains(&b))
}

pub fn references(message: &ReceivedMessage) -> Result<Vec<String>> {
    if !valid_message_id(&message.message_id) { return Err(fail("incoming message has no usable RFC Message-ID")); }
    let mut result = Vec::new();
    if let Some(value) = header(message,"references") {
        // Malformed untrusted references do not override the known direct parent.
        for id in value.split_whitespace().filter(|id|valid_message_id(id)).rev().take(19).collect::<Vec<_>>().into_iter().rev() {
            if id != message.message_id && !result.iter().any(|old|old==id) { result.push(id.to_owned()); }
        }
    }
    result.push(message.message_id.clone());
    while result.iter().map(|id|id.len()+1).sum::<usize>()>8192 { result.remove(0); }
    Ok(result)
}

pub fn received_at(message: &ReceivedMessage) -> Result<i64> {
    chrono::DateTime::parse_from_rfc3339(&message.created_at)
        .map(|time|time.timestamp()).map_err(|_|fail("received email has an invalid provider timestamp"))
}

/// Extract a top-posted answer. Ambiguous inline replies require a new complete
/// answer rather than silently grading text with some passages removed.
pub fn extract_answer(message: &ReceivedMessage) -> Result<String> {
    if message.attachments.iter().any(|a|a.content_disposition.as_deref()!=Some("inline")) {
        return Err(fail("Please paste your complete answer into the email body. Mentor does not read attached answers."));
    }
    let raw = match message.text.as_deref().filter(|text|!text.trim().is_empty()) {
        Some(text) => text.to_owned(),
        None => html_text(message.html.as_deref().unwrap_or_default())?,
    };
    if raw.len() > MAX_ANSWER_BYTES || raw.contains('\0') {
        return Err(fail("Please resend your answer as text under 128 KiB."));
    }
    let normalized = raw.replace("\r\n","\n").replace('\r',"\n");
    let mut answer = Vec::new();
    let mut quoted = false;
    let mut in_code = false;
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") { in_code = !in_code; }
        if !in_code {
            if trimmed == "-----Original Message-----"
                || trimmed == "________________________________"
                || (trimmed.starts_with("On ") && trimmed.ends_with("wrote:"))
                || line == "-- "
            { break; }
            if trimmed.starts_with('>') { quoted = true; continue; }
            if quoted && !trimmed.is_empty() {
                return Err(fail("Please send your complete answer above the quoted email. Mentor cannot separate this inline reply reliably."));
            }
        }
        answer.push(line);
    }
    let text = answer.join("\n").trim().to_owned();
    if text.is_empty() {
        return Err(fail("I could not find an answer in this reply. Please paste your complete answer above the quoted email."));
    }
    Ok(text)
}

fn html_text(source: &str) -> Result<String> {
    if source.len() > 1024*1024 { return Err(fail("Please resend your answer as plain text under 128 KiB.")); }
    let document = Html::parse_fragment(source);
    let mut output = String::new();
    let mut saw_quote = false;
    for node in document.tree.root().descendants() {
        let excluded = node.ancestors().any(|parent|match parent.value() {
            Node::Element(element) => matches!(element.name(),"script"|"style"|"head") || quote_element(element),
            _ => false,
        });
        if excluded { continue; }
        match node.value() {
            Node::Element(element) => {
                if quote_element(element) { saw_quote = true; continue; }
                if matches!(element.name(),"p"|"div"|"br"|"li"|"tr"|"pre"|"h1"|"h2"|"h3") { output.push('\n'); }
            }
            Node::Text(text) => {
                if saw_quote && !text.trim().is_empty() {
                    return Err(fail("Please send your complete answer above the quoted email. Mentor cannot separate this inline HTML reply reliably."));
                }
                output.push_str(text);
            }
            _ => {}
        }
    }
    Ok(output)
}

fn quote_element(element: &scraper::node::Element) -> bool {
    element.name()=="blockquote"
        || element.attr("class").is_some_and(|classes|classes.split_whitespace().any(|class|class=="gmail_quote"))
}
