# 44 — A trusted signature whose subject merely contains "Microsoft" is announced as Microsoft's.

Lane: lane-2-domain. Board: [../ROADMAP.md](../ROADMAP.md). This file is the entry until a spec absorbs it; the spec's PR deletes it.

**A trusted signature whose subject merely contains "Microsoft" is announced as
Microsoft's.** Spec: none yet → specify first.

Found on 2026-09-23 while stripping comments for C6 (PR #68), by reading the code; no such
certificate has been tried on a real machine.

In `crates/acter-shells/src/windows_signatures/trust.rs`, `verdict_for` maps a status of 0
from `WinVerifyTrust` to `Verdict::Trusted` and picks the signer from the certificate
subject's simple display name (`CertGetNameStringW`):
`Some(name) if name.contains(MICROSOFT) => Signer::Microsoft`, where `MICROSOFT` is
`"Microsoft"`. The doc comment on `MICROSOFT` said a prefix was matched; the code matches
anywhere in the name. Microsoft's own shells sign as `Microsoft Windows` and
`Microsoft Corporation`.

What follows from `Signer::Microsoft`, in `crates/acter-core/src/entities/signature_verdict.rs`:

- `Verdict::said` gives "This computer trusts this file's signature, and Microsoft signed
  it. There is nothing to decide before starting it."
- `Verdict::note` gives `None`, so nothing is said about the signer at connection.

`Signer::Other { name }` would instead give "… it was signed by {name}. Start it if that
is who you expect to have built it." and the note "signed by {name}". So a certificate that
chains to a root this machine trusts, issued to any subject whose name contains
"Microsoft", is presented to the listener as Microsoft's. Whether the file is trusted is
unaffected: `settled` is true for every trusted verdict, whoever signed it.
