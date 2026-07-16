# Contributing to CapyBite

Thanks for helping improve CapyBite.

## Before opening an issue

- Search existing issues for the same problem or idea.
- Include your macOS version and CapyBite version for bug reports.
- Do not post credentials, environment files, device identifiers, or private system information.

## Pull requests

1. Fork the repository and create a focused branch.
2. Keep unrelated formatting or dependency changes out of the pull request.
3. Run the validation commands:

   ```bash
   npm install
   npm run build
   cargo check --manifest-path src-tauri/Cargo.toml
   ```

4. Explain the problem, the chosen solution, and how it was tested.

By submitting code, you agree that your contribution is licensed under Apache-2.0. Contributions must not include third-party artwork or protected CapyBite brand assets without permission.
