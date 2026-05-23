use crate::SubmissionHandle;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FrameSyncReason {
    FrameBoundaryPresent,
    ReadbackCompletion,
    CompatibilityShim,
    ExplicitUserRequest,
    Shutdown,
    DeviceLossRecovery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameSyncReport {
    pub reason: FrameSyncReason,
    pub submitted: bool,
    pub waited: bool,
    pub presented: bool,
    pub submission: Option<SubmissionHandle>,
    pub notes: Vec<String>,
}

impl FrameSyncReport {
    pub(crate) fn submitted(reason: FrameSyncReason, submission: SubmissionHandle) -> Self {
        Self {
            reason,
            submitted: true,
            waited: false,
            presented: false,
            submission: Some(submission),
            notes: vec![
                "flush may wait for the previous frame fence before submitting new work"
                    .to_string(),
            ],
        }
    }

    pub(crate) fn waited(
        reason: FrameSyncReason,
        waited: bool,
        submission: Option<SubmissionHandle>,
    ) -> Self {
        Self {
            reason,
            submitted: false,
            waited,
            presented: false,
            submission,
            notes: if waited {
                vec!["wait blocked until the submitted frame completed".to_string()]
            } else {
                vec!["wait skipped because no submission exists for this frame".to_string()]
            },
        }
    }

    pub(crate) fn frame_boundary_present(
        reason: FrameSyncReason,
        submission: SubmissionHandle,
    ) -> Self {
        Self {
            reason,
            submitted: true,
            waited: true,
            presented: true,
            submission: Some(submission),
            notes: vec![
                "frame-boundary present submitted queued work, waited for completion, then presented"
                    .to_string(),
            ],
        }
    }
}
