<!--
MultiAgent Orchestrator Architecture — V1.3.6 CANDIDATE
DOCUMENT_AUTHORITY: CURRENT_NORMATIVE
Package: MultiAgent_Orchestrator_Architecture_V1_3_6_CANDIDATE
Issued by: BUILD-A1-BOOTSTRAP | Revision issued: 2026-08-16
Status: CANDIDATE — requires final independent review. NOT installed, NOT frozen.
Repository baseline unchanged: 01fe25bec239-collab/receipts @ 3c70f4d8bac1732058de50b383f0485ab4632de9
NEW_ARCHITECTURE_FREEZE_SHA: NOT ASSIGNED
FREEZE_READY: PENDING_FINAL_INDEPENDENT_REVIEW
Evidence authority: evidence/SOURCE_CLAIM_REGISTRY.json
Counts are DERIVED programmatically. Validator: evidence/validate_sources.py (non-zero exit on failure).
-->

# OPEN_CORE_COMMERCIAL_BOUNDARY

## The problem stated honestly

A public repository containing the complete Pro implementation plus `if entitlement == pro` is not enforcement. It is a speed bump with a comment explaining where to remove it.

## Options evaluated

**Option A — fully open source, honour system.** Simplest, maximum trust and contribution, zero enforcement. Viable only if Pro revenue is genuinely voluntary. Rejected as the primary model because it makes the paid tier unfundable.

**Option C — server-heavy orchestration.** Strongest control, but it conflicts directly with the product's own premises: source-code privacy, local Git and worktrees, customer-owned provider credentials, and offline-first operation. Choosing it purely for DRM would damage the product to protect its price. **Rejected.**

**Option B — open core. RECOMMENDED.**

```
PUBLIC / OSS                          PROPRIETARY / PAID
────────────                          ──────────────────
graph core + scheduler                distributed orchestration policy
FREE execution policy                 Model Intelligence execution
shared schemas                        multi-runtime routing implementation
host plugin shells                    independent A4 automation
basic provenance                      advanced failover
entitlement public verifier           advanced provenance
capability catalog                    other Pro execution modules
```

## Why open core fits this product specifically

The FREE tier is a genuinely useful standalone tool, so the public repository has real value and real contributors. The Pro tier's value is concentrated in orchestration policy and routing intelligence — code that is genuinely separable, and whose value is partly operational rather than purely algorithmic.

## Artifact inventory

| Class | Contents |
|---|---|
| `PUBLIC_ARTIFACT` | Graph core, FREE policy, schemas, plugin shells, basic provenance, entitlement **verifier**, capability catalog, docs |
| `PROPRIETARY_ARTIFACT` | Pro execution modules, distributed policy, routing implementation, advanced provenance |
| `SERVICE_SIDE_ARTIFACT` | Entitlement issuance, account/licence records, signing operation |
| `SECRET_NEVER_DISTRIBUTED` | Private signing keys, production service secrets, any provider credential |

Required and verified: `PUBLIC_PRIVATE_SIGNING_KEYS = 0` · `PUBLIC_PROVIDER_CREDENTIALS = 0` · `PUBLIC_PRODUCTION_LICENSE_SECRETS = 0`.

## Pro module delivery

Evaluated: separately installed signed local package · native compiled companion · private package-manager distribution · authenticated download after activation.

**Recommendation: authenticated download of a signed Pro module after activation**, verified against the same public key material as the entitlement. Plugin shells remain public; the Pro module is fetched and verified separately.

**Not assumed:** that Claude Code or Codex marketplaces can privately deliver arbitrary Pro binaries, or support paid distribution at all. That is `UNVERIFIED` (C-13) and the design does not depend on it.

## Version compatibility

`graph_schema_version` · `core_version` · `claude_plugin_version` · `codex_plugin_version` · `pro_module_version` · `entitlement_version` · `provider_registry_version`

Normal semantic compatibility applies; exact equality is not required. A Pro module incompatible with the core **fails fast with a clear message** rather than degrading unpredictably. A graph schema is never auto-downgraded destructively.

## Threat model — what this deters, and what it does not

**Deters:** casual copying, accidental Pro use without payment, organisational compliance risk (companies do not ship stripped licence checks), and the low-effort majority.

**Does not deter:** a determined reverse engineer with the Pro module on their machine. If Pro code is shipped locally, sufficiently motivated extraction is possible.

**No DRM claim is made.** The protection level is chosen as economically sensible for a developer tool: enough that paying is easier than not paying, not so much that the product becomes hostile to its own users. The tamper case (§64) is acknowledged rather than denied — a public shell modified to call a Pro endpoint directly is still refused by core admission, but a fully local Pro module cannot be made tamper-proof.
