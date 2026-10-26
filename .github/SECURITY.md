# Security

Report vulnerabilities privately through [GitHub's vulnerability reporting](https://github.com/bhark/eckermann/security/advisories/new), not as public issues.

Only the latest release gets fixes.

Every release ships with build provenance and a `SHA256SUMS` file. Verify a download with:

```sh
gh attestation verify <file> -R bhark/eckermann
```
