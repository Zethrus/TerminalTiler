use gtk::prelude::*;

use crate::model::assets::Runbook;

pub(crate) fn sync_runbook_selector(
    selector: &gtk::ComboBoxText,
    run_button: &gtk::Button,
    runbooks: &[Runbook],
    selected_id: Option<&str>,
) {
    selector.remove_all();
    selector.append(Some(""), "Runbook");
    for runbook in runbooks {
        selector.append(Some(&runbook.id), &runbook.name);
    }

    let selected_id = selected_id.unwrap_or_default();
    let keep_selection = !selected_id.is_empty()
        && runbooks
            .iter()
            .any(|runbook| runbook.id.as_str() == selected_id);
    selector.set_active_id(Some(if keep_selection { selected_id } else { "" }));
    run_button.set_sensitive(!runbooks.is_empty());
}
