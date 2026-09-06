//! Fixed-recipient plain-text messages and accepted-submission receipts.

pub use crate::AppError as Error;
use serde::{Deserialize, Serialize};

/// The exact message and optional caller-owned occurrence key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub subject: String,
    pub body: String,
    pub idempotency_key: Option<String>,
}

/// One caller-owned file attachment, already captured as exact bytes.
/// Only the filename and content are disclosed; local paths are not transmitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub filename: String,
    pub content: Vec<u8>,
}

/// Resend accepted the submission; this is not a final-delivery receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
}

impl std::fmt::Display for Receipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Accepted {}", self.id)
    }
}

/// Submit a message using Email's fixed addresses and existing environment credential.
/// The caller owns authorization and the exact payload associated with a supplied key.
///
/// # Errors
/// Returns the existing input, credential or bounded transport failure.
pub async fn send(message: &Message) -> Result<Receipt, Error> {
    send_with_attachments(message, &[]).await
}

/// Submit a fixed-recipient message with captured file attachments.
/// Attachment order, names, and bytes form part of the exact idempotent payload.
/// The caller owns authorization for disclosing each attachment as well as the body.
///
/// # Errors
/// Returns an invalid attachment filename, input, credential, or transport failure.
pub async fn send_with_attachments(
    message: &Message,
    attachments: &[Attachment],
) -> Result<Receipt, Error> {
    for attachment in attachments {
        crate::validate_attachment_filename(&attachment.filename)?;
    }
    let key = match &message.idempotency_key {
        Some(key) => crate::parse_idempotency_key(key).map_err(Error::new)?,
        None => crate::new_idempotency_key(),
    };
    let api_key = crate::resend_api_key()?;
    let id = crate::send_to(
        crate::RESEND_ENDPOINT,
        &api_key,
        &key,
        &message.subject,
        &message.body,
        attachments,
    )
    .await?;
    Ok(Receipt { id })
}
