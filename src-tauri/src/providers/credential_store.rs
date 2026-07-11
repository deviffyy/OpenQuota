#[cfg(target_os = "macos")]
pub fn read_generic_password(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
    use security_framework::passwords::{generic_password, PasswordOptions};

    match generic_password(PasswordOptions::new_generic_password(service, account)) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code() == -25_300 => Ok(None),
        Err(_) => Err("The macOS Keychain could not be read.".into()),
    }
}

#[cfg(target_os = "macos")]
pub fn write_generic_password(service: &str, account: &str, value: &[u8]) -> Result<(), String> {
    security_framework::passwords::set_generic_password(service, account, value)
        .map_err(|_| "The macOS Keychain could not be updated.".into())
}

#[cfg(target_os = "windows")]
pub fn read_generic_password(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
    use std::{ptr, slice};
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target = format!("{service}:{account}");
    let wide = target.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    let found = unsafe { CredReadW(wide.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if found == 0 {
        let code = std::io::Error::last_os_error().raw_os_error();
        return if code == Some(1168) {
            Ok(None)
        } else {
            Err("Windows Credential Manager could not be read.".into())
        };
    }
    if credential.is_null() {
        return Ok(None);
    }
    let value = unsafe {
        let credential_ref = &*credential;
        let bytes = slice::from_raw_parts(
            credential_ref.CredentialBlob,
            credential_ref.CredentialBlobSize as usize,
        )
        .to_vec();
        CredFree(credential.cast());
        bytes
    };
    Ok(Some(value))
}

#[cfg(target_os = "windows")]
pub fn write_generic_password(_service: &str, _account: &str, _value: &[u8]) -> Result<(), String> {
    Err("OpenQuota does not overwrite credentials owned by another Windows application.".into())
}

#[cfg(target_os = "linux")]
pub fn read_generic_password(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
    use std::collections::HashMap;

    use secret_service::{blocking::SecretService, EncryptionType};

    let secret_service = SecretService::connect(EncryptionType::Dh)
        .map_err(|_| "Linux Secret Service could not be reached.")?;
    let mut matches = secret_service
        .search_items(HashMap::from([("service", service), ("username", account)]))
        .map_err(|_| "Linux Secret Service could not be searched.")?;
    if let Some(item) = matches.unlocked.pop() {
        return item
            .get_secret()
            .map(Some)
            .map_err(|_| "Linux Secret Service item could not be read.".into());
    }
    let Some(item) = matches.locked.pop() else {
        return Ok(None);
    };
    item.unlock()
        .map_err(|_| "Linux Secret Service item could not be unlocked.")?;
    item.get_secret()
        .map(Some)
        .map_err(|_| "Linux Secret Service item could not be read.".into())
}

#[cfg(target_os = "linux")]
pub fn write_generic_password(service: &str, account: &str, value: &[u8]) -> Result<(), String> {
    use std::collections::HashMap;

    use secret_service::{blocking::SecretService, EncryptionType};

    let secret_service = SecretService::connect(EncryptionType::Dh)
        .map_err(|_| "Linux Secret Service could not be reached.")?;
    let mut matches = secret_service
        .search_items(HashMap::from([("service", service), ("username", account)]))
        .map_err(|_| "Linux Secret Service could not be searched.")?;
    let item = matches
        .unlocked
        .pop()
        .or_else(|| matches.locked.pop())
        .ok_or("The credential owned by the provider no longer exists.")?;
    item.unlock()
        .map_err(|_| "Linux Secret Service item could not be unlocked.")?;
    item.set_secret(value, "text/plain; charset=utf8")
        .map_err(|_| "Linux Secret Service item could not be updated.".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn read_generic_password(_service: &str, _account: &str) -> Result<Option<Vec<u8>>, String> {
    Ok(None)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn write_generic_password(_service: &str, _account: &str, _value: &[u8]) -> Result<(), String> {
    Err("The system credential store is unavailable on this platform.".into())
}

pub fn decode_go_keyring_value(value: &[u8]) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let text = std::str::from_utf8(value).ok()?.trim();
    let encoded = text.strip_prefix("go-keyring-base64:")?;
    String::from_utf8(STANDARD.decode(encoded).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD, Engine};

    use super::decode_go_keyring_value;

    #[test]
    fn decodes_go_keyring_wrapped_json() {
        let json = r#"{"access_token":"placeholder"}"#;
        let wrapped = format!("go-keyring-base64:{}", STANDARD.encode(json));
        assert_eq!(
            decode_go_keyring_value(wrapped.as_bytes()).as_deref(),
            Some(json)
        );
        assert!(decode_go_keyring_value(b"plain text").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_secret_service_round_trip_when_requested() {
        use std::collections::HashMap;

        use secret_service::{blocking::SecretService, EncryptionType};

        if std::env::var("OPENQUOTA_TEST_SECRET_SERVICE").as_deref() != Ok("1") {
            return;
        }
        let secret_service = SecretService::connect(EncryptionType::Dh).unwrap();
        let collection = secret_service
            .get_default_collection()
            .or_else(|_| secret_service.create_collection("OpenQuota Tests", "default"))
            .unwrap();
        collection.ensure_unlocked().unwrap();
        let item = collection
            .create_item(
                "OpenQuota Secret Service Test",
                HashMap::from([
                    ("service", "openquota-test-service"),
                    ("username", "openquota-test-account"),
                ]),
                b"first-value",
                true,
                "text/plain; charset=utf8",
            )
            .unwrap();

        assert_eq!(
            super::read_generic_password("openquota-test-service", "openquota-test-account")
                .unwrap()
                .as_deref(),
            Some(b"first-value".as_slice())
        );
        super::write_generic_password(
            "openquota-test-service",
            "openquota-test-account",
            b"second-value",
        )
        .unwrap();
        assert_eq!(item.get_secret().unwrap(), b"second-value");
        item.delete().unwrap();
    }
}
