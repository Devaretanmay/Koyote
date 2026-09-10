use super::types::{ImpactState, PatchTarget, UnresolvedCallsite};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyDecision {
    Approved,
    Refused { reason: String, quarantine: bool },
}

impl SafetyDecision {
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Unresolved, ambiguous, or unproven callsites are quarantined for human review.
pub struct SafetyPolicy;

impl SafetyPolicy {
    /// Determine if a callsite is permitted to enter the automated patching pipeline.
    pub fn evaluate_patch_eligibility(
        target: &PatchTarget,
        impact_state: &ImpactState,
    ) -> SafetyDecision {
        match impact_state {
            ImpactState::ConfirmedAffected => {
                // Verify patch semantics exist
                if target.upstream_change.trim().is_empty() {
                    return SafetyDecision::Refused {
                        reason: "Confirmed impact but missing patch transformation semantics.".into(),
                        quarantine: true,
                    };
                }
                SafetyDecision::Approved
            }
            ImpactState::ProvablyUnaffected => SafetyDecision::Refused {
                reason: "Callsite is provably unaffected by upstream contract drift. Zero modifications permitted.".into(),
                quarantine: false,
            },
            ImpactState::Unresolved => SafetyDecision::Refused {
                reason: "Uncertainty threshold exceeded. Unresolved references require human review; auto-patch is disabled.".into(),
                quarantine: true,
            },
        }
    }

    /// Invariant assertion: Unresolved references must NEVER enter the patch pipeline.
    pub fn assert_safe_to_patch(
        state: &ImpactState,
        unresolved_opt: Option<&UnresolvedCallsite>,
    ) -> Result<(), String> {
        match state {
            ImpactState::ConfirmedAffected => Ok(()),
            ImpactState::ProvablyUnaffected => Err(
                "SAFETY VIOLATION: Attempted to generate code for a provably unaffected callsite."
                    .into(),
            ),
            ImpactState::Unresolved => {
                let reason_str = unresolved_opt
                    .map(|u| format!("{:?}", u.reason))
                    .unwrap_or_else(|| "Unknown".into());
                Err(format!(
                    "SAFETY VIOLATION: Attempted to auto-patch unresolved reference (reason: {}). Policy strictly prohibits automated changes to unverified references.",
                    reason_str
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::autopatch::types::UncertaintyReason;

    #[test]
    fn policy_approves_confirmed_affected_with_semantics() {
        let target = PatchTarget {
            file_path: "src/billing.ts".into(),
            line_numbers: vec![10],
            reason: "POST /v1/charges changed".into(),
            upstream_change: "Parameter 'amount' type changed from 'integer' to 'string'".into(),
        };
        let decision =
            SafetyPolicy::evaluate_patch_eligibility(&target, &ImpactState::ConfirmedAffected);
        assert_eq!(decision, SafetyDecision::Approved);
        assert!(SafetyPolicy::assert_safe_to_patch(&ImpactState::ConfirmedAffected, None).is_ok());
    }

    #[test]
    fn policy_blocks_unresolved_references() {
        let target = PatchTarget {
            file_path: "src/billing.ts".into(),
            line_numbers: vec![10],
            reason: "Import Stripe".into(),
            upstream_change: "".into(),
        };
        let decision = SafetyPolicy::evaluate_patch_eligibility(&target, &ImpactState::Unresolved);
        assert!(matches!(
            decision,
            SafetyDecision::Refused {
                quarantine: true,
                ..
            }
        ));

        let unresolved = UnresolvedCallsite {
            reason: UncertaintyReason::DynamicMethodChain,
            file_path: "src/client.ts".into(),
            line_number: 42,
            source_text: "client[method]()".into(),
            provider: "Stripe".into(),
            inferred_operation: None,
            confidence_evidence: "Dynamic indexing".into(),
            why_autofix_disabled: "Dynamic resolution cannot be verified statically".into(),
        };
        let res = SafetyPolicy::assert_safe_to_patch(&ImpactState::Unresolved, Some(&unresolved));
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("SAFETY VIOLATION"));
    }

    #[test]
    fn policy_blocks_provably_unaffected() {
        let target = PatchTarget {
            file_path: "src/checkout.ts".into(),
            line_numbers: vec![5],
            reason: "POST /v1/checkout/sessions".into(),
            upstream_change: "".into(),
        };
        let decision =
            SafetyPolicy::evaluate_patch_eligibility(&target, &ImpactState::ProvablyUnaffected);
        assert!(matches!(
            decision,
            SafetyDecision::Refused {
                quarantine: false,
                ..
            }
        ));
        assert!(
            SafetyPolicy::assert_safe_to_patch(&ImpactState::ProvablyUnaffected, None).is_err()
        );
    }
}
