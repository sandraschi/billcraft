//! Peppol BIS sender: SBDH package, SMP lookup, one signed AS4 post.
//!
//! The signature covers the SignedInfo bytes as they are written. A row becomes
//! `accepted` only when the response is an AS4 receipt for that message id.

use crate::BillCoreError;
use base64::Engine;
use billcraft_store::{BillStore, PeppolOutbox};
use flate2::write::GzEncoder;
use flate2::Compression;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::sha2::Sha256;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use std::io::Write;
use std::time::Duration;

pub struct SigningMaterial {
    pub key_pem: String,
    pub token_b64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeppolOutcome {
    pub message_id: String,
    pub status: String,
    pub detail: String,
    pub package: String,
}

pub trait PeppolTransport {
    fn lookup_endpoint(&self, scheme: &str, participant: &str) -> Result<String, BillCoreError>;
    fn post(
        &self,
        endpoint: &str,
        body: &[u8],
        content_type: &str,
    ) -> Result<Vec<u8>, BillCoreError>;
}

pub fn send_live(store: &BillStore, invoice_id: &str) -> Result<PeppolOutcome, BillCoreError> {
    let row = store.peppol_prepare(invoice_id)?;
    let Some(live) = LivePeppol::from_env()? else {
        return Ok(prepared_outcome(
            &row,
            "Package was not sent. Set BILLCRAFT_AP_CERT, BILLCRAFT_AP_KEY, and BILLCRAFT_SMP_URL.",
        ));
    };
    send_prepared(store, row, &live, &live.material)
}

pub fn send_with<T: PeppolTransport>(
    store: &BillStore,
    invoice_id: &str,
    transport: &T,
    material: &SigningMaterial,
) -> Result<PeppolOutcome, BillCoreError> {
    let row = store.peppol_prepare(invoice_id)?;
    send_prepared(store, row, transport, material)
}

fn prepared_outcome(row: &PeppolOutbox, detail: &str) -> PeppolOutcome {
    PeppolOutcome {
        message_id: row.message_id.clone(),
        status: row.status.clone(),
        detail: detail.to_string(),
        package: row.package.clone(),
    }
}

fn send_prepared<T: PeppolTransport>(
    store: &BillStore,
    row: PeppolOutbox,
    transport: &T,
    material: &SigningMaterial,
) -> Result<PeppolOutcome, BillCoreError> {
    let (sender, scheme, participant) = receiver_from_package(&row.package)?;
    let endpoint = match transport.lookup_endpoint(&scheme, &participant) {
        Ok(url) => url,
        Err(error) => {
            let updated = store.update_peppol_outbox(
                row.id,
                "failed",
                None,
                None,
                Some(&error.to_string()),
            )?;
            return Ok(outcome_from(&updated, &error.to_string()));
        }
    };
    let (body, content_type) =
        as4_message(&row, material, &sender, &format!("{scheme}:{participant}"))?;
    let _ = store.update_peppol_outbox(row.id, "sent", Some(&endpoint), None, None)?;
    let response = match transport.post(&endpoint, &body, &content_type) {
        Ok(bytes) => bytes,
        Err(error) => {
            let updated = store.update_peppol_outbox(
                row.id,
                "failed",
                Some(&endpoint),
                None,
                Some(&error.to_string()),
            )?;
            return Ok(outcome_from(&updated, &error.to_string()));
        }
    };
    let text = String::from_utf8_lossy(&response).to_string();
    if is_as4_receipt(&text, &row.message_id) {
        let updated =
            store.update_peppol_outbox(row.id, "accepted", Some(&endpoint), Some(&text), None)?;
        return Ok(outcome_from(&updated, "AS4 receipt stored."));
    }
    let updated = store.update_peppol_outbox(
        row.id,
        "failed",
        Some(&endpoint),
        Some(&text),
        Some("response is not an AS4 receipt for this message"),
    )?;
    Ok(outcome_from(
        &updated,
        "response is not an AS4 receipt for this message",
    ))
}

fn outcome_from(row: &PeppolOutbox, detail: &str) -> PeppolOutcome {
    PeppolOutcome {
        message_id: row.message_id.clone(),
        status: row.status.clone(),
        detail: detail.to_string(),
        package: row.package.clone(),
    }
}

fn receiver_from_package(package: &str) -> Result<(String, String, String), BillCoreError> {
    let marker = "iso6523-actorid-upis\">";
    let mut rest = package;
    let mut ids = Vec::new();
    for _ in 0..2 {
        let Some(start) = rest.find(marker) else {
            break;
        };
        let after = &rest[start + marker.len()..];
        let Some(end) = after.find('<') else {
            break;
        };
        ids.push(after[..end].trim().to_string());
        rest = &after[end..];
    }
    let (Some(sender), Some(receiver)) = (ids.first(), ids.get(1)) else {
        return Err(BillCoreError::Validation(
            "prepared package has no sender and receiver".into(),
        ));
    };
    let Some((scheme, id)) = receiver.split_once(':') else {
        return Err(BillCoreError::Validation(
            "receiver participant is missing a scheme".into(),
        ));
    };
    Ok((sender.clone(), scheme.to_string(), id.to_string()))
}

fn as4_message(
    row: &PeppolOutbox,
    material: &SigningMaterial,
    sender: &str,
    receiver: &str,
) -> Result<(Vec<u8>, String), BillCoreError> {
    let gz = gzip(row.package.as_bytes())?;
    let cid = format!("payload-{}", row.id);
    let user_message = user_message(&row.message_id, &cid, sender, receiver);
    let digest = sha256_b64(user_message.as_bytes());
    let signed_info = signed_info(&digest);
    let signature = sign_pkcs1(&material.key_pem, signed_info.as_bytes())?;
    let soap = soap_envelope(&user_message, &signed_info, &signature, &material.token_b64);
    let boundary = format!("billcraft-{}", row.id);
    let mut body = Vec::new();
    write!(
        &mut body,
        "--{boundary}\r\nContent-Type: application/soap+xml; charset=UTF-8\r\nContent-ID: <soap>\r\n\r\n{soap}\r\n--{boundary}\r\nContent-Type: application/gzip\r\nContent-Transfer-Encoding: binary\r\nContent-ID: <{cid}>\r\n\r\n"
    )
    .map_err(BillCoreError::Io)?;
    body.extend_from_slice(&gz);
    write!(&mut body, "\r\n--{boundary}--\r\n").map_err(BillCoreError::Io)?;
    let content_type =
        format!("multipart/related; boundary=\"{boundary}\"; type=\"application/soap+xml\"");
    Ok((body, content_type))
}

fn user_message(message_id: &str, cid: &str, sender: &str, receiver: &str) -> String {
    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    format!(
        "<eb:UserMessage xmlns:eb=\"http://docs.oasis-open.org/ebxml-msg/ebms/v3.0/ns/core/200704/\" wsu:Id=\"msg-{id}\" xmlns:wsu=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd\">\
<eb:MessageInfo><eb:Timestamp>{stamp}</eb:Timestamp><eb:MessageId>{id}</eb:MessageId></eb:MessageInfo>\
<eb:PartyInfo>\
<eb:From><eb:PartyId type=\"urn:oasis:names:tc:ebcore:partyid-type:unregistered:iso6523-actorid-upis\">{sender}</eb:PartyId><eb:Role>http://docs.oasis-open.org/ebxml-msg/ebms/v3.0/ns/core/200704/initiator</eb:Role></eb:From>\
<eb:To><eb:PartyId type=\"urn:oasis:names:tc:ebcore:partyid-type:unregistered:iso6523-actorid-upis\">{receiver}</eb:PartyId><eb:Role>http://docs.oasis-open.org/ebxml-msg/ebms/v3.0/ns/core/200704/responder</eb:Role></eb:To>\
</eb:PartyInfo>\
<eb:CollaborationInfo>\
<eb:Service type=\"cenbii-procid-ubl\">urn:fdc:peppol.eu:2017:poacc:billing:01:1.0</eb:Service>\
<eb:Action>urn:oasis:names:specification:ubl:schema:xsd:Invoice-2</eb:Action>\
<eb:ConversationId>{id}</eb:ConversationId>\
</eb:CollaborationInfo>\
<eb:PayloadInfo><eb:PartInfo href=\"cid:{cid}\"><eb:PartProperties>\
<eb:Property name=\"CompressionType\">application/gzip</eb:Property>\
<eb:Property name=\"MimeType\">application/xml</eb:Property>\
</eb:PartProperties></eb:PartInfo></eb:PayloadInfo>\
</eb:UserMessage>",
        id = xml_text(message_id),
        cid = xml_text(cid),
        sender = xml_text(sender),
        receiver = xml_text(receiver),
    )
}

fn signed_info(digest_b64: &str) -> String {
    format!(
        "<ds:SignedInfo xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\">\
<ds:CanonicalizationMethod Algorithm=\"http://www.w3.org/2001/10/xml-exc-c14n#\"/>\
<ds:SignatureMethod Algorithm=\"http://www.w3.org/2001/04/xmldsig-more#rsa-sha256\"/>\
<ds:Reference URI=\"#msg\"><ds:DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha256\"/>\
<ds:DigestValue>{digest_b64}</ds:DigestValue></ds:Reference></ds:SignedInfo>"
    )
}

fn soap_envelope(
    user_message: &str,
    signed_info: &str,
    signature_b64: &str,
    token_b64: &str,
) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<soap:Envelope xmlns:soap=\"http://www.w3.org/2003/05/soap-envelope\">\
<soap:Header>\
<eb:Messaging xmlns:eb=\"http://docs.oasis-open.org/ebxml-msg/ebms/v3.0/ns/core/200704/\" soap:mustUnderstand=\"true\">{user_message}</eb:Messaging>\
<wsse:Security xmlns:wsse=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd\">\
<wsse:BinarySecurityToken EncodingType=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary\" ValueType=\"http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-x509-token-profile-1.0#X509v3\">{token_b64}</wsse:BinarySecurityToken>\
<ds:Signature xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\">{signed_info}<ds:SignatureValue>{signature_b64}</ds:SignatureValue></ds:Signature>\
</wsse:Security>\
</soap:Header><soap:Body/></soap:Envelope>"
    )
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, BillCoreError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}

fn sha256_b64(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256 as Sha};
    let digest = Sha::digest(bytes);
    base64::engine::general_purpose::STANDARD.encode(digest)
}

fn sign_pkcs1(key_pem: &str, bytes: &[u8]) -> Result<String, BillCoreError> {
    let key = RsaPrivateKey::from_pkcs8_pem(key_pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(key_pem))
        .map_err(|e| BillCoreError::Validation(format!("access-point key: {e}")))?;
    let signing = SigningKey::<Sha256>::new(key);
    let signature = signing.sign(bytes);
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()))
}

pub fn is_as4_receipt(body: &str, message_id: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let receipt = lower.contains("<eb:receipt")
        || lower.contains(":receipt>")
        || lower.contains("nonrepudiationinformation");
    receipt && body.contains(message_id)
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct LivePeppol {
    smp_base: String,
    material: SigningMaterial,
}

impl LivePeppol {
    fn from_env() -> Result<Option<Self>, BillCoreError> {
        let cert = std::env::var("BILLCRAFT_AP_CERT").ok();
        let key = std::env::var("BILLCRAFT_AP_KEY").ok();
        let smp = std::env::var("BILLCRAFT_SMP_URL").ok();
        match (cert, key, smp) {
            (Some(cert), Some(key), Some(smp)) => Ok(Some(Self::load(&cert, &key, &smp)?)),
            _ => Ok(None),
        }
    }

    fn load(cert_path: &str, key_path: &str, smp_base: &str) -> Result<Self, BillCoreError> {
        let key_pem = std::fs::read_to_string(key_path)?;
        let cert_pem = std::fs::read_to_string(cert_path)?;
        let token_b64 = pem_body(&cert_pem).ok_or_else(|| {
            BillCoreError::Validation("BILLCRAFT_AP_CERT is not a PEM certificate".into())
        })?;
        let _ = sign_pkcs1(&key_pem, b"billcraft-key-check")?;
        Ok(Self {
            smp_base: smp_base.trim_end_matches('/').to_string(),
            material: SigningMaterial { key_pem, token_b64 },
        })
    }
}

impl PeppolTransport for LivePeppol {
    fn lookup_endpoint(&self, scheme: &str, participant: &str) -> Result<String, BillCoreError> {
        let url = format!(
            "{}/iso6523-actorid-upis::{}:{}/services/busdox-docid-qns::{}",
            self.smp_base,
            percent_encode(scheme),
            percent_encode(participant),
            percent_encode("urn:oasis:names:specification:ubl:schema:xsd:Invoice-2")
        );
        let body = http_get(&url)?;
        endpoint_uri(&body)
            .ok_or_else(|| BillCoreError::Validation("SMP response has no EndpointURI".into()))
    }

    fn post(
        &self,
        endpoint: &str,
        body: &[u8],
        content_type: &str,
    ) -> Result<Vec<u8>, BillCoreError> {
        http_post(endpoint, body, content_type)
    }
}

fn pem_body(pem: &str) -> Option<String> {
    let mut data = String::new();
    let mut inside = false;
    for line in pem.lines() {
        let line = line.trim();
        if line.starts_with("-----BEGIN ") {
            inside = true;
            continue;
        }
        if line.starts_with("-----END ") {
            break;
        }
        if inside {
            data.push_str(line);
        }
    }
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

fn endpoint_uri(xml: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let key = "endpointuri";
    let start = lower.find(key)?;
    let after_name = start + key.len();
    let close = xml[after_name..].find('>')?;
    let value_at = after_name + close + 1;
    let end = xml[value_at..].find('<')?;
    let value = xml[value_at..value_at + end].trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        Some(value.to_string())
    } else {
        None
    }
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn http_get(url: &str) -> Result<String, BillCoreError> {
    agent()
        .get(url)
        .call()
        .map_err(|e| BillCoreError::Validation(format!("SMP lookup failed: {e}")))?
        .into_string()
        .map_err(BillCoreError::Io)
}

fn http_post(url: &str, body: &[u8], content_type: &str) -> Result<Vec<u8>, BillCoreError> {
    let text = agent()
        .post(url)
        .set("Content-Type", content_type)
        .send_bytes(body)
        .map_err(|e| BillCoreError::Validation(format!("AS4 post failed: {e}")))?
        .into_string()
        .map_err(BillCoreError::Io)?;
    Ok(text.into_bytes())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BillService;
    use billcraft_store::{BillStore, InvoiceStatus, XrParty};
    use chrono::NaiveDate;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::RsaPrivateKey;
    use std::cell::RefCell;

    struct Stub {
        receipt: bool,
        seen: RefCell<Vec<u8>>,
    }

    impl PeppolTransport for Stub {
        fn lookup_endpoint(
            &self,
            scheme: &str,
            participant: &str,
        ) -> Result<String, BillCoreError> {
            if scheme == "0204" && participant == "04011000-12345-03" {
                Ok("https://ap.example.test/as4".into())
            } else {
                Err(BillCoreError::Validation(format!(
                    "unknown participant {scheme}:{participant}"
                )))
            }
        }

        fn post(
            &self,
            _endpoint: &str,
            body: &[u8],
            _content_type: &str,
        ) -> Result<Vec<u8>, BillCoreError> {
            self.seen.borrow_mut().extend_from_slice(body);
            if !self.receipt {
                return Ok(b"<html>no</html>".to_vec());
            }
            let text = String::from_utf8_lossy(body);
            let id = text
                .split("<eb:MessageId>")
                .nth(1)
                .and_then(|rest| rest.split("</eb:MessageId>").next())
                .unwrap_or("missing");
            Ok(format!(
                "<eb:SignalMessage xmlns:eb=\"http://docs.oasis-open.org/ebxml-msg/ebms/v3.0/ns/core/200704/\"><eb:MessageInfo><eb:RefToMessageId>{id}</eb:RefToMessageId></eb:MessageInfo><eb:Receipt><eb:NonRepudiationInformation/></eb:Receipt></eb:SignalMessage>"
            )
            .into_bytes())
        }
    }

    fn material() -> Result<SigningMaterial, String> {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).map_err(|e| e.to_string())?;
        let key_pem = key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| e.to_string())?
            .to_string();
        let public_der = key
            .to_public_key()
            .to_public_key_der()
            .map_err(|e| e.to_string())?;
        let token_b64 = base64::engine::general_purpose::STANDARD.encode(public_der.as_bytes());
        Ok(SigningMaterial { key_pem, token_b64 })
    }

    fn issued(store: &BillStore) -> Result<(), String> {
        store
            .create_invoice(
                "INV-S",
                "buyer",
                "Buyer GmbH",
                "buyer@example.test",
                None,
                None,
                None,
                NaiveDate::from_ymd_opt(2026, 11, 1).ok_or("date")?,
                NaiveDate::from_ymd_opt(2026, 11, 15).ok_or("date")?,
                None,
            )
            .map_err(|e| e.to_string())?;
        store
            .add_item("INV-S", "Work", 2.0, 5000, 0.20)
            .map_err(|e| e.to_string())?;
        store
            .update_status("INV-S", InvoiceStatus::Issued)
            .map_err(|e| e.to_string())?;
        let party = |scheme: &str, id: &str, reference: Option<&str>| XrParty {
            name: "Party".into(),
            street: "Street 1".into(),
            city: "Wien".into(),
            postcode: "1010".into(),
            country: "AT".into(),
            email: "party@example.test".into(),
            vat_id: Some("ATU12345678".into()),
            phone: Some("+431111".into()),
            iban: Some("AT611904300234573201".into()),
            buyer_reference: reference.map(str::to_string),
            peppol_scheme: Some(scheme.into()),
            peppol_id: Some(id.into()),
        };
        store
            .save_xrechnung_party("seller", &party("9930", "ATU12345678", None))
            .map_err(|e| e.to_string())?;
        store
            .save_xrechnung_party("INV-S", &party("0204", "04011000-12345-03", Some("REF-1")))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn receipt_marks_accepted_and_html_marks_failed() -> Result<(), String> {
        let material = material()?;
        let store = BillStore::open_in_memory().map_err(|e| e.to_string())?;
        issued(&store)?;
        let service = BillService::new(store, None);
        let ok = Stub {
            receipt: true,
            seen: RefCell::new(Vec::new()),
        };
        let accepted = service
            .peppol_send_with("INV-S", &ok, &material)
            .map_err(|e| e.to_string())?;
        if accepted.status != "accepted" {
            return Err(accepted.status);
        }
        let seen = ok.seen.borrow();
        let posted = String::from_utf8_lossy(&seen);
        if !posted.contains("<ds:SignatureValue>") || !posted.contains("application/gzip") {
            return Err("AS4 body is missing the signature or the gzip part".into());
        }
        if !is_as4_receipt("<html>no</html>", &accepted.message_id) && accepted.status == "accepted"
        {
            let store = BillStore::open_in_memory().map_err(|e| e.to_string())?;
            issued(&store)?;
            let service = BillService::new(store, None);
            let bad = Stub {
                receipt: false,
                seen: RefCell::new(Vec::new()),
            };
            let failed = service
                .peppol_send_with("INV-S", &bad, &material)
                .map_err(|e| e.to_string())?;
            if failed.status != "failed" {
                return Err(failed.status);
            }
        }
        Ok(())
    }
}
