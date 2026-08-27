# Basic Workdir Sandbox (Experimental)

This document describes the `sandbox-basic-workdir` experiment. It is intended
for local testing, not as a production security policy.

## Intended behavior

- Bash starts in the selected workdir.
- The workdir and Heddle runtime root are writable.
- Installed developer tools and their runtime libraries are readable/executable.
- Outbound network access is allowed for Cargo, Rustup, Git, and other tooling.
- The normal subprocess environment is preserved, including `HOME` and `PATH`,
  except Heddle and recognised credential variables are removed before Bash
  starts. `SSH_AUTH_SOCK` remains available for agent-backed signing.
- Sensitive paths are explicitly denied in the Seatbelt profile.

The environment scrub removes all `HEDDLE_*` variables and common credential
names/suffixes such as `*_TOKEN`, `*_KEY`, `*_API_KEY`, `*_SECRET`, `*_PASSWORD`,
`*_CREDENTIAL(S)`, `GH_TOKEN`, and `DOCKER_AUTH_CONFIG`. This is defence in
depth, not proof that every secret uses a conventional variable name.

## Sensitive-path deny list

The initial deny list covers:

- `$HOME/.ssh`
- `$HOME/.aws`
- `$HOME/.gnupg`
- `$HOME/.config/gcloud`
- `$HOME/.config/gh`
- `$HOME/.npmrc`
- `$HOME/.netrc`
- `/etc/master.passwd`
- `/etc/passwd`
- `/etc/shadow`
- `/private/var/db/dslocal`

Add paths to the `sensitive` list in `src/tools/bash.rs` as the testing surface
reveals more host data that should not be visible. Keep deny rules after broad
runtime-read rules if Seatbelt precedence in the target OS requires it, and add
a regression test for each new sensitive path.

## Important limitations

This is deliberately broad: a process in the workdir can execute available
programs and make network requests. It is not equivalent to a container or a
credential-free environment. In particular, do not use it with untrusted code
until the deny list and Seatbelt precedence have been validated on the target
macOS version.

The branch is separate from `main` so the experiment can be discarded or
reviewed without changing the existing policy.
