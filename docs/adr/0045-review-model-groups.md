# Review uses explicit model groups

The default model is a preference for standalone actions. Treating every installed CLI as permission to distribute a Review made that setting misleading. It also selected models through provider fallback order, which can retain an obsolete model from an older configuration.

Review uses explicit Fast, Standard and Advanced model groups. The groups define which exact models may receive the diff. Fast covers triage and Quick, Standard covers ordinary review, and Advanced covers Deep and the arbiter. The default provider is preferred when it belongs to the chosen group. Standalone actions keep their default model.

The plan is captured at the start of a Review. Settings changes apply to later runs. Deep confirmation rejects a changed allowed plan rather than starting different models from the ones estimated. A group with no installed choices produces a configuration error instead of falling back to an unselected provider.

This makes distribution predictable at the cost of maintaining deliberate model choices. Installation does not establish authentication. The CLI still reports sign-in failures. Findings retain the provider provenance described in ADR 0044.
