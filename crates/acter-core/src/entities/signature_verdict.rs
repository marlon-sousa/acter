//! Entity/value: what verifying a file's signature found, and the sentence a listener hears
//! about it.

/// What this computer said about a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Trusted {
        signer: Signer,
    },
    Untrusted {
        fault: Fault,
    },
    /// The check could not run.
    Unverifiable {
        /// A whole sentence, spoken after the verdict's own.
        why: String,
    },
}

/// Who signed a file this machine trusts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signer {
    Microsoft,
    Apple,
    Other {
        /// The certificate subject, as `CertGetNameStringW` renders it.
        name: String,
    },
}

impl Signer {
    pub fn vendor(&self) -> &str {
        match self {
            Self::Microsoft => "Microsoft",
            Self::Apple => "Apple",
            Self::Other { name } => name,
        }
    }
}

/// The ways this computer can verify a signature and refuse it; each `signer` is `None`
/// when the certificate named nobody readable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// No embedded signature, and no catalog on this machine claims its hash.
    NotSigned,
    /// Signed with no certificate, as Apple silicon requires of every locally built binary.
    AdHoc,
    Tampered,
    UntrustedRoot {
        signer: Option<String>,
    },
    Revoked {
        signer: Option<String>,
    },
    /// Expired when it signed, with no timestamp.
    Expired {
        signer: Option<String>,
    },
}

impl Verdict {
    /// Only a trusted file starts without asking.
    pub fn settled(&self) -> bool {
        matches!(self, Self::Trusted { .. })
    }

    pub fn signer(&self) -> Option<String> {
        match self {
            Self::Trusted {
                signer: signer @ (Signer::Microsoft | Signer::Apple),
            } => Some(signer.vendor().to_owned()),
            Self::Trusted {
                signer: Signer::Other { name },
            } => Some(name.clone()),
            Self::Untrusted {
                fault:
                    Fault::UntrustedRoot { signer }
                    | Fault::Revoked { signer }
                    | Fault::Expired { signer },
            } => signer.clone(),
            Self::Untrusted {
                fault: Fault::NotSigned | Fault::AdHoc | Fault::Tampered,
            }
            | Self::Unverifiable { .. } => None,
        }
    }

    /// Whole sentences ending in what to do next.
    pub fn said(&self) -> String {
        match self {
            Self::Trusted {
                signer: signer @ (Signer::Microsoft | Signer::Apple),
            } => format!(
                "This computer trusts this file's signature, and {} signed it. There is \
                 nothing to decide before starting it.",
                signer.vendor()
            ),
            Self::Trusted {
                signer: Signer::Other { name },
            } => format!(
                "This computer trusts this file's signature, and it was signed by {name}. \
                 Start it if that is who you expect to have built it."
            ),
            Self::Untrusted {
                fault: Fault::NotSigned,
            } => "Nothing has signed this file, so there is no record of who built it or \
                  whether it has been changed since. Start it only if you know how it got \
                  there."
                .to_owned(),
            Self::Untrusted {
                fault: Fault::AdHoc,
            } => "This file signed itself, so it has not been altered since it was built and \
                  there is no record of who built it. That is what a program compiled on this \
                  computer, or installed by a package manager, normally looks like. Start it \
                  only if you know how it got there."
                .to_owned(),
            Self::Untrusted {
                fault: Fault::Tampered,
            } => "This file has been changed since it was signed, so it is not the file \
                  whoever signed it produced. Do not start it unless you know what changed \
                  it."
            .to_owned(),
            Self::Untrusted {
                fault: Fault::UntrustedRoot { signer },
            } => format!(
                "This file is signed {}, and this computer does not trust whoever issued \
                 that signature. Start it only if you know why that certificate is not \
                 trusted here.",
                by(signer.as_deref())
            ),
            Self::Untrusted {
                fault: Fault::Revoked { signer },
            } => format!(
                "{} has been revoked, which is what happens when a signing key is found to \
                 have been stolen or misused. Do not start it.",
                certificate(signer.as_deref())
            ),
            Self::Untrusted {
                fault: Fault::Expired { signer },
            } => format!(
                "{} had already expired, and nothing recorded when the signing happened. \
                 Start it only if you know where the file came from.",
                certificate(signer.as_deref())
            ),
            Self::Unverifiable { why } => format!(
                "Acter could not check who signed this file. {why} Start it only if you know \
                 where it came from."
            ),
        }
    }

    /// Said once at connection about a file started anyway, and `None` when Microsoft or
    /// Apple signed it.
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Trusted {
                signer: Signer::Microsoft | Signer::Apple,
            } => None,
            Self::Trusted {
                signer: Signer::Other { name },
            } => Some(format!("signed by {name}")),
            Self::Untrusted {
                fault: Fault::NotSigned,
            } => Some("started although nothing has signed it".to_owned()),
            Self::Untrusted {
                fault: Fault::AdHoc,
            } => Some("started although nothing records who built it".to_owned()),
            Self::Untrusted {
                fault: Fault::Tampered,
            } => Some("started although it has been changed since it was signed".to_owned()),
            Self::Untrusted {
                fault: Fault::UntrustedRoot { .. },
            } => Some("started although this computer does not trust who signed it".to_owned()),
            Self::Untrusted {
                fault: Fault::Revoked { .. },
            } => Some("started although its signing certificate has been revoked".to_owned()),
            Self::Untrusted {
                fault: Fault::Expired { .. },
            } => Some("started although its signing certificate had expired".to_owned()),
            Self::Unverifiable { .. } => {
                Some("started without Acter being able to check who signed it".to_owned())
            }
        }
    }
}

fn by(signer: Option<&str>) -> String {
    signer.map_or_else(|| "by somebody".to_owned(), |name| format!("by {name}"))
}

fn certificate(signer: Option<&str>) -> String {
    signer.map_or_else(
        || "The certificate this file was signed with".to_owned(),
        |name| format!("The certificate {name} signed this file with"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_verdict() -> Vec<Verdict> {
        vec![
            Verdict::Trusted {
                signer: Signer::Microsoft,
            },
            Verdict::Trusted {
                signer: Signer::Apple,
            },
            Verdict::Trusted {
                signer: Signer::Other {
                    name: "Contoso Corporation".to_owned(),
                },
            },
            Verdict::Untrusted {
                fault: Fault::NotSigned,
            },
            Verdict::Untrusted {
                fault: Fault::AdHoc,
            },
            Verdict::Untrusted {
                fault: Fault::Tampered,
            },
            Verdict::Untrusted {
                fault: Fault::UntrustedRoot {
                    signer: Some("Contoso Corporation".to_owned()),
                },
            },
            Verdict::Untrusted {
                fault: Fault::Revoked { signer: None },
            },
            Verdict::Untrusted {
                fault: Fault::Expired { signer: None },
            },
            Verdict::Unverifiable {
                why: "This file cannot be opened, so there is nothing to check.".to_owned(),
            },
        ]
    }

    #[test]
    fn every_verdict_speaks_whole_sentences() {
        for verdict in every_verdict() {
            let said = verdict.said();
            let first = said
                .chars()
                .next()
                .expect("a verdict always says something");

            assert!(
                first.is_uppercase(),
                "a spoken verdict starts a sentence: {said}"
            );
            assert!(
                said.ends_with('.'),
                "and ends where a reader should pause: {said}"
            );
            assert!(
                said.split_whitespace().count() >= 10,
                "and says what happened rather than naming it: {said}"
            );
        }
    }

    #[test]
    fn every_verdict_names_what_to_do_next() {
        for verdict in every_verdict() {
            let said = verdict.said();

            assert!(
                said.contains("Start it")
                    || said.contains("Do not start it")
                    || said.contains("nothing to decide"),
                "a verdict says what to do about itself: {said}"
            );
        }
    }

    #[test]
    fn no_two_verdicts_are_said_the_same_way() {
        let said: Vec<String> = every_verdict().iter().map(Verdict::said).collect();

        for (index, one) in said.iter().enumerate() {
            for other in &said[index + 1..] {
                assert_ne!(one, other, "each verdict is said differently");
            }
        }
    }

    #[test]
    fn only_a_trusted_file_is_started_without_a_word() {
        for verdict in every_verdict() {
            assert_eq!(
                verdict.settled(),
                matches!(verdict, Verdict::Trusted { .. }),
                "{verdict:?}"
            );
        }
    }

    #[test]
    fn a_file_microsoft_signed_adds_nothing_to_what_connecting_says() {
        assert_eq!(
            Verdict::Trusted {
                signer: Signer::Microsoft
            }
            .note(),
            None
        );
    }

    #[test]
    fn everything_a_user_had_to_agree_to_is_said_when_it_starts() {
        for verdict in every_verdict() {
            if verdict.settled()
                && matches!(
                    verdict.signer().as_deref(),
                    Some("Microsoft") | Some("Apple")
                )
            {
                continue;
            }
            let note = verdict
                .note()
                .unwrap_or_else(|| panic!("{verdict:?} is worth a clause"));
            assert!(!note.is_empty());
            assert!(
                !note.ends_with('.'),
                "a note is a clause appended to a sentence, not a sentence: {note}"
            );
        }
    }

    #[test]
    fn who_signed_it_can_be_read_on_its_own() {
        assert_eq!(
            Verdict::Trusted {
                signer: Signer::Other {
                    name: "Contoso Corporation".to_owned()
                }
            }
            .signer()
            .as_deref(),
            Some("Contoso Corporation")
        );
        assert_eq!(
            Verdict::Untrusted {
                fault: Fault::NotSigned
            }
            .signer(),
            None,
            "nothing signed it, so there is nobody to name"
        );
    }
}
