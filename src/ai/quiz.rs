use serde::{Deserialize, Serialize};

// ── .er/quiz.json ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErQuiz {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub questions: Vec<QuizQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizQuestion {
    pub id: String,
    #[serde(default = "default_quiz_level")]
    pub level: u8,
    #[serde(default)]
    pub category: String,
    pub text: String,
    #[serde(default)]
    pub options: Option<Vec<QuizOption>>,
    #[serde(default)]
    pub freeform: bool,
    #[serde(default)]
    pub expected_reasoning: String,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub related_file: String,
    pub related_hunk: Option<usize>,
    #[serde(default)]
    pub related_lines: Option<(usize, usize)>,
}

fn default_quiz_level() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizOption {
    pub label: char,
    pub text: String,
    #[serde(default)]
    pub is_correct: bool,
}

impl ErQuiz {
    /// Get questions related to a specific file
    #[allow(dead_code)]
    pub fn questions_for_file(&self, file: &str) -> Vec<&QuizQuestion> {
        self.questions
            .iter()
            .filter(|q| q.related_file == file)
            .collect()
    }
}
