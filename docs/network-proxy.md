# Network Proxy Design

Seatbelt is the hard default-deny boundary for direct network access. The current MVP mediates common network tools and evaluates destination policy, but it does not claim complete hostname-level enforcement for arbitrary binaries.

The next enforcement layer is a supervisor-owned loopback HTTP/HTTPS proxy:

- The child receives proxy variables pointing only at the supervisor socket.
- Seatbelt permits loopback access to that socket and denies direct egress.
- The supervisor resolves DNS and evaluates host, port, method, path, and secret-use policy outside the sandbox.
- Every request is logged with destination, decision, command, and policy hash.
- TLS is passed through by default; optional inspection requires a separate explicit trust configuration.

The proxy must be paired with a PF or Network Extension backstop before it is considered a complete replacement for direct egress denial.

