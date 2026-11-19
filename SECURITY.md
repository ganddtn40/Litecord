# Security policy

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's
[private vulnerability reporting](https://github.com/ganddtn40/litecord/security/advisories/new)
and include:

- the version from **Settings → About → Copy version**,
- what an attacker can do and what they need to start,
- the smallest reproduction you can manage.

You should get an answer within a few days. Once a fix is ready, the advisory
is published with credit unless you ask to stay anonymous.

## Scope

Litecord talks straight to Discord and keeps its session token in the operating
system's credential store. Reports about token handling, the update signature
check, the embedded protocol parsers, and anything that lets a message or a
server take over the client are especially useful.

Third-party services and Discord itself are out of scope.
