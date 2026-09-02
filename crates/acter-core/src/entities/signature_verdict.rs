//! Entity/value: what verifying a file's signature found, and the sentence a listener hears
//! about it.
//!
//! **It stopped saying "Windows" with M2.** Every sentence here opened with the name of one
//! operating system, and macOS reads them too — so what a listener is told is now "this
//! computer", which is true wherever there is a computer to trust something (spec M2,
//! decision 6).
//!
//! **Three outcomes rather than two, and the third is the one that matters** (spec B5.7,
//! decision 4). Trusted and untrusted are the obvious pair; unverifiable is what an
//! execution alias that cannot be opened produces, what a revocation answer that never comes
//! produces, and what an unfamiliar Windows status produces. Reporting either of the other
//! two in its place would be a lie to somebody who cannot see the file — quietly trusted, or
//! quietly condemned.
//!
//! **Never a gate** (decision 6). A verdict is a sentence and a question, not a filter: a
//! self-built pwsh, a corporate re-signed build, a damaged catalog database and an offline
//! revocation check are all legitimate and all common, and hiding a user's shell for any of
//! them teaches the lesson B5.4 refused to teach — that Acter cannot see the shell they are
//! looking straight at.
//!
//! What the check buys is real and narrower than it sounds: it defeats `PATH`-order
//! hijacking, which is cheap and common, and it tells the user *before* the program runs
//! rather than after. It is not a sandbox, and it says nothing about what a correctly signed
//! program then does.

/// What this computer said about a file, in the form a listener is told it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// This computer verified the signature and trusts the chain behind it.
    Trusted {
        /// Who signed it — **a second question and a different call** (decision 5), because
        /// "trusted and signed by Microsoft" and "trusted and signed by somebody else" are
        /// different sentences and a verdict that could not tell them apart would be no use
        /// on a machine with a corporate root.
        signer: Signer,
    },
    /// This computer verified the signature and will not trust it.
    Untrusted {
        /// Which of the ways it can be untrusted this was, because they need different
        /// sentences: a file nobody signed and a file somebody changed after signing are
        /// not the same news.
        fault: Fault,
    },
    /// This computer could not answer, so neither can Acter.
    Unverifiable {
        /// Why not, as a whole clause. It is read aloud after the sentence it belongs to,
        /// so it is a sentence of its own rather than a code.
        why: String,
    },
}

/// Who signed a file this machine trusts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signer {
    /// Microsoft — every shell Windows ships, through its catalog, and PowerShell 7 through
    /// its own embedded signature.
    Microsoft,
    /// Apple — every shell macOS ships, whose leaf certificate is Apple's own "Software
    /// Signing" and whose chain reaches the Apple Root CA (spec M2, decision 6, measured
    /// 2026-09-01 against all seven entries in this Mac's `/etc/shells`).
    ///
    /// **The vendor of the operating system is a variant rather than a name**, for
    /// [`Self::Microsoft`]'s reason: "trusted, and the company that made this computer signed
    /// it" is a different sentence from "trusted, and somebody else did", and a verdict that
    /// could not tell them apart would announce something on every ordinary connection.
    Apple,
    /// Somebody else this machine's trust store accepts: a corporate re-signed build, a
    /// vendor, a root an administrator installed.
    Other {
        /// The certificate subject, as `CertGetNameStringW` renders it.
        name: String,
    },
}

impl Signer {
    /// What this signer is called in a sentence, for the two that are an operating system's
    /// own vendor.
    ///
    /// [`Self::Other`] answers with its own name, which is the certificate subject as the
    /// platform renders it.
    pub fn vendor(&self) -> &str {
        match self {
            Self::Microsoft => "Microsoft",
            Self::Apple => "Apple",
            Self::Other { name } => name,
        }
    }
}

/// The ways this computer can verify a signature and refuse it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Nothing signed this file: no embedded signature, and no catalog on this machine
    /// claims its hash.
    NotSigned,
    /// The file signed itself: there is a signature, it records the file's own hashes, and
    /// there is no certificate and therefore nobody behind it.
    ///
    /// **A fourth way of being untrusted, added by M2, because macOS makes it ordinary.**
    /// Apple silicon requires every executable to carry at least an ad-hoc signature, so
    /// this is what a locally built or Homebrew-installed shell has — common, legitimate, and
    /// not the same news as "nothing signed this". Telling a listener nothing had signed it
    /// would be false about the half that is true: the file has not been altered since it was
    /// built. What is missing is who built it.
    AdHoc,
    /// The file does not hash to what the signature says it should, so it is not the file
    /// whoever signed it produced.
    Tampered,
    /// It is signed, and the chain does not reach a root this computer trusts.
    UntrustedRoot {
        /// Who it claims to be signed by, when that could be read at all.
        signer: Option<String>,
    },
    /// The certificate it was signed with has been revoked.
    Revoked {
        /// Who it claims to be signed by, when that could be read at all.
        signer: Option<String>,
    },
    /// The certificate had expired by the time it signed this, and nothing timestamped it.
    Expired {
        /// Who it claims to be signed by, when that could be read at all.
        signer: Option<String>,
    },
}

impl Verdict {
    /// Whether this is a verdict nobody needs to act on.
    ///
    /// **The one question the connection asks of a verdict** (decision 6): a trusted file is
    /// started without a word, and everything else is put to the person in front of the
    /// window before anything runs.
    pub fn settled(&self) -> bool {
        matches!(self, Self::Trusted { .. })
    }

    /// Who signed it, when anything could be read about that.
    ///
    /// Said on its own, rather than only inside the sentence, so a dialog can put it
    /// somewhere a listener can read character by character (spec B5.7, accessibility
    /// checklist).
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

    /// What a listener is told, as whole sentences ending in what to do next.
    ///
    /// **Every verdict has one, including the ones nobody hears** (spec B5.7, definition of
    /// done). A verdict with no sentence is a verdict somebody later renders as a code.
    pub fn said(&self) -> String {
        match self {
            Self::Trusted {
                signer: signer @ (Signer::Microsoft | Signer::Apple),
            } => format!(
                "This computer trusts this file's signature, and {} signed it. There is \
                 nothing to decide before starting it.",
                signer.vendor()
            ),
            // **No "rather than" clause, since M2.** It used to read "rather than by
            // Microsoft", which is a contrast a Mac cannot draw and which said nothing the
            // first half had not: what the listener needs is who did sign it.
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

    /// The clause said once, at connection, about a file that was started anyway.
    ///
    /// **`None` for a file the operating system's own vendor signed, because a verdict
    /// nobody needs to act on is not an announcement** (spec B5.7, accessibility checklist).
    /// Connecting to a normally installed shell says exactly what it says today — and that
    /// is what M2 had to be true of on a Mac, where every shell in `/etc/shells` is signed by
    /// Apple and a note on each would be a sentence at every connection.
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

/// "by Microsoft" or "by somebody", so the sentence reads either way round.
fn by(signer: Option<&str>) -> String {
    signer.map_or_else(|| "by somebody".to_owned(), |name| format!("by {name}"))
}

/// The subject of the two sentences that are about the certificate rather than the file,
/// naming who used it when that could be read.
fn certificate(signer: Option<&str>) -> String {
    signer.map_or_else(
        || "The certificate this file was signed with".to_owned(),
        |name| format!("The certificate {name} signed this file with"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every verdict this product can reach, so a variant cannot be added without deciding
    /// what it says.
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

    /// The rule CLAUDE.md makes a domain requirement: these are read aloud, so each is a
    /// whole sentence rather than a label or a status code.
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

    /// **The half a status code cannot carry** (spec B5.7, definition of done): every
    /// sentence ends by naming what to do next, because a listener who has just been told
    /// something is wrong with a file is being asked to decide.
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

    /// Told apart by what a listener hears rather than by a discriminant. The pair this is
    /// really about is trusted-and-Microsoft against trusted-and-somebody-else (decision 5),
    /// which are the same trust and very different news.
    #[test]
    fn no_two_verdicts_are_said_the_same_way() {
        let said: Vec<String> = every_verdict().iter().map(Verdict::said).collect();

        for (index, one) in said.iter().enumerate() {
            for other in &said[index + 1..] {
                assert_ne!(one, other, "each verdict is said differently");
            }
        }
    }

    /// The one question the connection asks: is this a verdict anybody has to act on.
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

    /// **A verdict nobody needs to act on is not an announcement** (accessibility
    /// checklist): connecting to a normally installed shell says exactly what it says today.
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

    /// And everything else does say something, because starting it was a decision the user
    /// made and they should hear what they agreed to (accessibility checklist).
    #[test]
    fn everything_a_user_had_to_agree_to_is_said_when_it_starts() {
        for verdict in every_verdict() {
            // The two vendors of an operating system this product runs on: a shell they
            // signed is the ordinary case, and the test above is what pins its silence.
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

    /// The signer travels on its own as well as inside the sentence, so a dialog can put it
    /// somewhere it can be read character by character.
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
