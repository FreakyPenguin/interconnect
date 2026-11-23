//! Presentation snapshot of SDK-aggregated MSC3381 polls.
use matrix_sdk::ruma::{UserId, events::poll::start::PollKind};
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent};

#[derive(Clone, Debug)]
pub struct Poll {
    pub question: String,
    pub answers: Vec<Answer>,
    pub ended: bool,
    pub disclosed: bool,
    pub max_selections: u64,
}
#[derive(Clone, Debug)]
pub struct Answer {
    pub id: String,
    pub text: String,
    pub votes: usize,
    pub selected: bool,
}
impl Poll {
    pub fn validate_vote(&self, answers: &[String]) -> Result<(), String> {
        let unique: std::collections::HashSet<_> = answers.iter().collect();
        if self.ended
            || answers.is_empty()
            || answers.len() as u64 > self.max_selections
            || unique.len() != answers.len()
            || answers
                .iter()
                .any(|id| !self.answers.iter().any(|a| &a.id == id))
        {
            return Err("Choose valid answers in an open poll".into());
        }
        Ok(())
    }

    pub fn from_content(content: &TimelineItemContent, own_user: &UserId) -> Option<Self> {
        let TimelineItemContent::MsgLike(content) = content else {
            return None;
        };
        let MsgLikeKind::Poll(poll) = &content.kind else {
            return None;
        };
        let results = poll.results();
        let ended = results.end_time.is_some();
        let disclosed = ended || results.kind == PollKind::Disclosed;
        Some(Self {
            question: results.question,
            answers: results
                .answers
                .into_iter()
                .map(|answer| {
                    let voters = results.votes.get(&answer.id);
                    Answer {
                        id: answer.id,
                        text: answer.text,
                        votes: if disclosed {
                            voters.map_or(0, Vec::len)
                        } else {
                            0
                        },
                        selected: voters.is_some_and(|v| v.iter().any(|u| u == own_user.as_str())),
                    }
                })
                .collect(),
            ended,
            disclosed,
            max_selections: results.max_selections,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poll() -> Poll {
        Poll {
            question: "Lunch?".into(),
            answers: ["soup", "salad"]
                .into_iter()
                .map(|id| Answer {
                    id: id.into(),
                    text: id.into(),
                    votes: 0,
                    selected: false,
                })
                .collect(),
            ended: false,
            disclosed: true,
            max_selections: 1,
        }
    }
    #[test]
    fn votes_require_open_poll_known_distinct_answers_and_selection_limit() {
        let mut poll = poll();
        assert!(poll.validate_vote(&["soup".into()]).is_ok());
        for answers in [
            vec![],
            vec!["unknown".into()],
            vec!["soup".into(), "salad".into()],
            vec!["soup".into(), "soup".into()],
        ] {
            assert!(poll.validate_vote(&answers).is_err());
        }
        poll.max_selections = 2;
        assert!(poll.validate_vote(&["soup".into(), "salad".into()]).is_ok());
        poll.ended = true;
        assert!(poll.validate_vote(&["soup".into()]).is_err());
    }
}
