# Public source policy

This repository contains only the local CapyBite macOS application needed to build and review its system-monitoring behavior.

The public source must not contain:

- private service or database implementation details;
- production endpoints, credentials, tokens, or environment values;
- signing certificates, provisioning profiles, or release credentials;
- private operational documentation, logs, exports, or user data;
- unpublished service integrations or internal deployment automation.

The public repository is maintained as a separate, reviewed source tree. It is not an automatic mirror of any private development repository. Every future update must be reviewed as a public release change before it is pushed.

If restricted material is discovered, stop distribution, remove it from the repository and its history, rotate any affected credentials, and publish a security notice when appropriate.
