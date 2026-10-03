# Telete instructions

Semantics-Project: telete

Keep Telete independent of the installed Cell CI manager. Use only Telete's
state, refs, worktrees, service identity, and executable. Do not import Python
CI modules or invoke the existing manager, broker, validator, or deployment
wrappers. Do not submit this implementation to CI or install it without a new
user request.

Use ordinary Rust data types and runtime correlation checks. Do not add checked
evidence types, a public CI client, or an Iatreion status integration.

Keep provider authority with each provider. Use the supported Rust interfaces
for Nucleus, Bazaar, Email, Chancery, and Usher. Keep source and operational
records private. Test changes with focused local Rust checks.
