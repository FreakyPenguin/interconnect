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
