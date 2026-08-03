# Team Policy Trust Model

Project policy is convenient but untrusted. A checked-in policy may request capabilities, but it cannot grant host credentials, unrestricted network, or access outside the workspace by itself.

The planned team-policy layer uses a signed canonical TOML document:

1. The organization publishes a policy document and public signing key through a trusted distribution channel.
2. Safe Agent verifies the signature before treating team defaults as trusted input.
3. User-private policy may narrow team policy but cannot broaden protected-path or secret restrictions.
4. Repo policy may narrow or request capabilities, but broadening changes remain approval-gated.
5. The effective policy records source, hash, signer, and verification result in the session audit log.

Unsigned or invalid team policy is treated as advisory input and never as an authority grant. Key rotation and revocation must be explicit before this becomes a production feature.

