//! Offline hash-type identification → hashcat mode.
//!
//! Recognises the formats that actually show up on CTF boxes (secretsdump,
//! Responder, kerberoast/AS-REP, /etc/shadow, web app DBs, archives) by prefix
//! and shape. Bare hex digests are ambiguous by nature, so those return every
//! plausible candidate, most likely first.

/// One candidate interpretation of a hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashKind {
    pub name: &'static str,
    /// hashcat `-m` mode; `None` when hashcat can't do it.
    pub mode: Option<u32>,
}

const fn k(name: &'static str, mode: u32) -> HashKind {
    HashKind { name, mode: Some(mode) }
}

impl HashKind {
    /// John the Ripper (jumbo) `--format=` name, when there is one.
    pub fn john(&self) -> Option<&'static str> {
        Some(match self.mode {
            Some(1000) => "NT",
            Some(0) => "raw-md5",
            Some(900) => "raw-md4",
            Some(3000) => "LM",
            Some(100) => "raw-sha1",
            Some(1300) => "raw-sha224",
            Some(1400) => "raw-sha256",
            Some(10800) => "raw-sha384",
            Some(1700) => "raw-sha512",
            Some(13100) => "krb5tgs",
            Some(19600) => "krb5tgs-aes128",
            Some(19700) => "krb5tgs-aes256",
            Some(18200) => "krb5asrep",
            Some(7500) => "krb5pa-md5",
            Some(2100) => "mscash2",
            Some(3200) => "bcrypt",
            Some(1800) => "sha512crypt",
            Some(7400) => "sha256crypt",
            Some(500) | Some(1600) => "md5crypt",
            Some(400) => "phpass",
            Some(10000) => "django",
            Some(5600) => "netntlmv2",
            Some(5500) => "netntlm",
            Some(300) => "mysql-sha1",
            Some(13600) => "zip",
            Some(17200) => "pkzip",
            Some(13400) => "keepass",
            Some(22921) => "ssh",
            Some(9600) => "office",
            Some(10500) => "pdf",
            Some(11600) => "7z",
            Some(13000) => "rar5",
            Some(111) => "salted-sha1",
            None if self.name.starts_with("yescrypt") => "crypt",
            None if self.name == "Argon2" => "argon2",
            _ => return None,
        })
    }
}

/// Normalise what the user pasted into the hash hashcat should get:
/// `LM:NT` pairs (secretsdump / pwdump lines) reduce to the NT half.
pub fn crackable(hash: &str) -> String {
    let h = hash.trim();
    // pwdump: user:rid:LM:NT:::
    let parts: Vec<&str> = h.split(':').collect();
    if parts.len() >= 4 && is_hex(parts[2], 32) && is_hex(parts[3], 32) {
        return parts[3].to_lowercase();
    }
    if parts.len() == 2 && is_hex(parts[0], 32) && is_hex(parts[1], 32) {
        return parts[1].to_lowercase();
    }
    h.to_string()
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Candidate hash types, best guess first. Empty when nothing matched.
pub fn identify(raw: &str) -> Vec<HashKind> {
    let h = crackable(raw);
    let h = h.as_str();
    let lower = h.to_ascii_lowercase();

    // ── prefixed formats (unambiguous) ──
    const PREFIXED: &[(&str, HashKind)] = &[
        ("$krb5tgs$23$", k("Kerberos 5 TGS-REP etype 23 (kerberoast)", 13100)),
        ("$krb5tgs$17$", k("Kerberos 5 TGS-REP etype 17 (AES128)", 19600)),
        ("$krb5tgs$18$", k("Kerberos 5 TGS-REP etype 18 (AES256)", 19700)),
        ("$krb5asrep$23$", k("Kerberos 5 AS-REP etype 23 (asreproast)", 18200)),
        ("$krb5pa$23$", k("Kerberos 5 AS-REQ Pre-Auth etype 23", 7500)),
        ("$dcc2$", k("Domain Cached Credentials 2 (DCC2)", 2100)),
        ("$2a$", k("bcrypt", 3200)),
        ("$2b$", k("bcrypt", 3200)),
        ("$2y$", k("bcrypt", 3200)),
        ("$6$", k("sha512crypt (Linux shadow)", 1800)),
        ("$5$", k("sha256crypt (Linux shadow)", 7400)),
        ("$1$", k("md5crypt", 500)),
        ("$apr1$", k("Apache apr1 md5", 1600)),
        ("$p$", k("phpass (WordPress / phpBB)", 400)),
        ("$h$", k("phpass (WordPress / phpBB)", 400)),
        ("pbkdf2_sha256$", k("Django PBKDF2-SHA256", 10000)),
        ("sha1$", k("Django SHA1", 124)),
        ("$pbkdf2-sha256$", k("PBKDF2-HMAC-SHA256 (passlib)", 20300)),
        ("$pbkdf2-sha512$", k("PBKDF2-HMAC-SHA512 (passlib)", 20200)),
        ("$zip2$", k("WinZip AES", 13600)),
        ("$pkzip2$", k("PKZIP (compressed)", 17200)),
        ("$pkzip$", k("PKZIP (compressed)", 17200)),
        ("$keepass$", k("KeePass", 13400)),
        ("$sshng$", k("SSH private key (sshng)", 22921)),
        ("$office$", k("MS Office 2007+", 9600)),
        ("$pdf$", k("PDF 1.4-1.6", 10500)),
        ("$7z$", k("7-Zip", 11600)),
        ("$rar5$", k("RAR5", 13000)),
        ("$mysql$", k("MySQL $A$ (sha256crypt)", 7401)),
        ("{ssha}", k("LDAP SSHA1", 111)),
        ("{sha}", k("LDAP SHA1 (nsldap)", 101)),
    ];
    for (prefix, kind) in PREFIXED {
        if lower.starts_with(prefix) {
            return vec![*kind];
        }
    }
    if lower.starts_with("$y$") {
        return vec![HashKind { name: "yescrypt (Linux shadow)", mode: None }];
    }
    if lower.starts_with("$argon2") {
        return vec![HashKind { name: "Argon2", mode: None }];
    }

    // ── NetNTLM (Responder / ntlmrelayx): user::domain:chal:resp:blob ──
    let parts: Vec<&str> = h.split(':').collect();
    if parts.len() >= 6 && parts[1].is_empty() {
        if is_hex(parts[3], 16) && parts[4].len() == 32 && parts[5].len() > 32 {
            return vec![k("NetNTLMv2", 5600)];
        }
        if parts[3].len() == 48 || parts[4].len() == 48 {
            return vec![k("NetNTLMv1", 5500)];
        }
    }

    // ── MySQL 4.1+: *<40 hex> ──
    if let Some(rest) = h.strip_prefix('*') {
        if is_hex(rest, 40) {
            return vec![k("MySQL 4.1+", 300)];
        }
    }

    // ── bare hex digests: ambiguous, ordered by CTF likelihood ──
    if h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return match h.len() {
            32 => vec![k("NTLM", 1000), k("MD5", 0), k("MD4", 900), k("LM (half)", 3000)],
            40 => vec![k("SHA1", 100), k("RIPEMD-160", 6000)],
            56 => vec![k("SHA224", 1300)],
            64 => vec![k("SHA256", 1400), k("SHA3-256", 17400)],
            96 => vec![k("SHA384", 10800)],
            128 => vec![k("SHA512", 1700), k("SHA3-512", 17600), k("Whirlpool", 6100)],
            _ => vec![],
        };
    }

    // salted md5/sha1 as hash:salt (common in web DB dumps)
    if parts.len() == 2 && is_hex(parts[0], 32) {
        return vec![k("md5($pass.$salt)", 10), k("md5($salt.$pass)", 20)];
    }
    if parts.len() == 2 && is_hex(parts[0], 40) {
        return vec![k("sha1($pass.$salt)", 110), k("sha1($salt.$pass)", 120)];
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(h: &str) -> Option<u32> {
        identify(h).first().and_then(|k| k.mode)
    }

    #[test]
    fn common_ctf_hashes() {
        // secretsdump line → NT hash
        let line = "Administrator:500:aad3b435b51404eeaad3b435b51404ee:31d6cfe0d16ae931b73c59d7e0c089c0:::";
        assert_eq!(crackable(line), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(mode(line), Some(1000));
        assert_eq!(mode("aad3b435b51404eeaad3b435b51404ee:31d6cfe0d16ae931b73c59d7e0c089c0"), Some(1000));
        assert_eq!(mode("$krb5tgs$23$*svc$DOM$x*$abc$def"), Some(13100));
        assert_eq!(mode("$krb5asrep$23$user@DOM:abc$def"), Some(18200));
        assert_eq!(mode("$6$salt$Zm9v"), Some(1800));
        assert_eq!(mode("$2y$10$abcdefghijklmnopqrstuu"), Some(3200));
        assert_eq!(mode("$P$BabcdefghijklmnopqrstuvwxyzABCD"), Some(400));
        assert_eq!(mode("5f4dcc3b5aa765d61d8327deb882cf99"), Some(1000)); // NTLM first, MD5 second
        assert_eq!(identify("5f4dcc3b5aa765d61d8327deb882cf99")[1].mode, Some(0));
        assert_eq!(mode("5baa61e4c9b93f3f0682250b6cf8331b7ee68fd8"), Some(100));
        assert_eq!(mode("*2470C0C06DEE42FD1618BB99005ADCA2EC9D1E19"), Some(300));
        let ntv2 = format!("admin::CORP:1122334455667788:{}:{}", "a".repeat(32), "0101".repeat(20));
        assert_eq!(mode(&ntv2), Some(5600));
        assert_eq!(identify("$y$j9T$abc$def")[0].mode, None);
        assert!(identify("hello world").is_empty());
        assert_eq!(identify(line)[0].john(), Some("NT"));
        assert_eq!(identify("$y$j9T$abc$def")[0].john(), Some("crypt"));
    }
}
