# Nucleus feature contracts

The detailed runtime contract lives in Nucleus's installed Chancery feature
publication. Read the product overview and one feature, or resolve the complete
operating contract:

```sh
chancery product nucleus
chancery show nucleus.invocation
chancery resolve nucleus.execution.operate
```

| Subject | Chancery identity |
| --- | --- |
| Jobs, identity, admission, and lifecycle | `nucleus.jobs` |
| Request format, permissions, and harness support | `nucleus.invocation` |
| Schemas, toolsets, and durable mailbox | `nucleus.requester-tools` |
| Status, output, and history | `nucleus.output` |
| Credentials and account access | `nucleus.authentication` |
| Weekly quota admission | `nucleus.quota` |
| Service readiness, maintenance, and recovery | `nucleus.service` |

Use `chancery show ID` for a focused page. Use `chancery resolve ID` to include
required contracts and inspect compatibility and gaps. This page preserves the
old documentation entry point; feature details have one home in the provider
bundle. Installed documentation does not establish live service readiness.
