//! Versões `MAJOR.MINOR.PATCH` com pré-lançamento opcional (`-alpha.N`, `-beta.N`, `-rc.N`).
//! Mesma regra de `scripts/version.mjs`: nada além disso é aceito.

use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Alpha,
    Beta,
    Rc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<(Channel, u64)>,
}

fn number(s: &str) -> Option<u64> {
    // Sem zeros à esquerda e só dígitos ASCII.
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

impl Version {
    /// `x.y.z` ou `x.y.z-rc.1`. Não aceita prefixo `v`, espaços nem metadados de build.
    pub fn parse(text: &str) -> Option<Version> {
        let (core, pre) = match text.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (text, None),
        };
        let mut parts = core.split('.');
        let (major, minor, patch) = (number(parts.next()?)?, number(parts.next()?)?, number(parts.next()?)?);
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => None,
            Some(p) => {
                let (name, n) = p.split_once('.')?;
                let channel = match name {
                    "alpha" => Channel::Alpha,
                    "beta" => Channel::Beta,
                    "rc" => Channel::Rc,
                    _ => return None,
                };
                Some((channel, number(n)?))
            }
        };
        Some(Version { major, minor, patch, pre })
    }

    /// Como vem de uma tag do Git: `v0.2.0`. Exige o prefixo `v`.
    pub fn from_tag(tag: &str) -> Option<Version> {
        Version::parse(tag.strip_prefix('v')?)
    }

    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            match (self.pre, other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater, // a versão final vem depois do pré-lançamento
                (Some(_), None) => Ordering::Less,
                (Some((ca, na)), Some((cb, nb))) => (ca as u8, na).cmp(&(cb as u8, nb)),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some((c, n)) = self.pre {
            let name = match c {
                Channel::Alpha => "alpha",
                Channel::Beta => "beta",
                Channel::Rc => "rc",
            };
            write!(f, "-{name}.{n}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("should parse: {s}"))
    }

    #[test]
    fn accepts_exactly_what_the_release_scripts_accept() {
        for ok in ["0.1.0", "1.2.3", "10.20.30", "1.0.0-rc.1", "2.0.0-beta.12", "3.1.4-alpha.0"] {
            assert!(Version::parse(ok).is_some(), "{ok}");
        }
        for bad in [
            "1", "1.2", "v1.2.3", "01.2.3", "1.2.3.4", "1.2.3-rc", "1.2.3-foo.1", "1.2.3+build", "", " 1.2.3",
            "1.2.3 ", "1.2.3-rc.01", "-1.2.3", "1.-2.3", "1.2.3-", "a.b.c", "1.2.99999999999999999999999",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn tags_need_the_v_prefix() {
        assert_eq!(Version::from_tag("v0.2.0"), Some(v("0.2.0")));
        assert_eq!(Version::from_tag("0.2.0"), None);
        assert_eq!(Version::from_tag("V0.2.0"), None);
        assert_eq!(Version::from_tag("vv0.2.0"), None);
    }

    #[test]
    fn ordering_is_numeric_and_prereleases_come_first() {
        assert!(v("0.2.0") > v("0.1.9"));
        assert!(v("0.10.0") > v("0.9.0"), "numeric, not lexical");
        assert!(v("1.0.0") > v("1.0.0-rc.1"));
        assert!(v("1.0.0-rc.2") > v("1.0.0-rc.1"));
        assert!(v("1.0.0-rc.1") > v("1.0.0-beta.9"));
        assert!(v("1.0.0-beta.1") > v("1.0.0-alpha.9"));
        assert!(v("1.0.1-alpha.1") > v("1.0.0"), "core version dominates the pre-release tag");
        assert_eq!(v("1.2.3").cmp(&v("1.2.3")), Ordering::Equal);
    }

    #[test]
    fn display_round_trips() {
        for s in ["0.1.0", "1.0.0-rc.1", "2.3.4-beta.12"] {
            assert_eq!(v(s).to_string(), s);
        }
    }
}
