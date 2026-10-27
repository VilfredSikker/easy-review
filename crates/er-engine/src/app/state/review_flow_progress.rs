use super::{App, OverlayData};

impl App {
    pub fn open_review_flow_progress(&mut self, er_dir: &str) {
        if let Some(flow) = self.review_flow_for(er_dir).cloned() {
            self.overlay = Some(OverlayData::ReviewFlowProgress {
                er_dir: er_dir.to_owned(),
                flow: Box::new(flow),
                selected: 0,
                detail_scroll: 0,
            });
        }
    }

    pub(super) fn refresh_review_flow_progress(&mut self) {
        let Some(OverlayData::ReviewFlowProgress { er_dir, .. }) = &self.overlay else {
            return;
        };
        let Some(latest) = self.review_flow_for(er_dir).cloned() else {
            return;
        };
        if let Some(OverlayData::ReviewFlowProgress {
            flow,
            selected,
            detail_scroll,
            ..
        }) = &mut self.overlay
        {
            let selected_id = flow.tasks.get(*selected).map(|task| &task.id);
            let next = selected_id
                .and_then(|id| latest.tasks.iter().position(|task| &task.id == id))
                .unwrap_or_else(|| (*selected).min(latest.tasks.len().saturating_sub(1)));
            if next != *selected {
                *detail_scroll = 0;
            }
            *selected = next;
            **flow = latest;
        }
    }
}
