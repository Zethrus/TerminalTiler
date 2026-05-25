use std::rc::Rc;

use adw::prelude::*;

use crate::model::assets::{CliSnippet, TemplateVariableValues};
use crate::ui::icons::{self, name as icon_name};

pub(crate) type BeforeSnippetPopoverPopup = Rc<dyn Fn(&gtk::Popover) -> bool>;
pub(crate) type ExecuteSnippet =
    Rc<dyn Fn(&CliSnippet, TemplateVariableValues, &gtk::Popover) -> Result<(), String>>;

#[derive(Clone)]
pub(crate) struct SnippetPopoverInput {
    pub snippets_provider: Rc<dyn Fn() -> Vec<CliSnippet>>,
    pub before_popup: Option<BeforeSnippetPopoverPopup>,
    pub execute: ExecuteSnippet,
}

pub(crate) fn install(button: &gtk::Button, input: SnippetPopoverInput) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("snippet-popover");
    popover.set_autohide(true);
    popover.set_has_arrow(true);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(button);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    popover.set_child(Some(&content));

    refresh_snippet_list(
        &content,
        &popover,
        Rc::new((input.snippets_provider)()),
        &input,
    );

    {
        let popover = popover.clone();
        let content = content.clone();
        let input = input.clone();
        button.connect_clicked(move |_| {
            if let Some(before_popup) = &input.before_popup
                && !before_popup(&popover)
            {
                return;
            }
            refresh_snippet_list(
                &content,
                &popover,
                Rc::new((input.snippets_provider)()),
                &input,
            );
            if popover.is_visible() {
                popover.popdown();
            } else {
                popover.popup();
            }
        });
    }

    popover
}

fn refresh_snippet_list(
    content: &gtk::Box,
    popover: &gtk::Popover,
    snippets: Rc<Vec<CliSnippet>>,
    input: &SnippetPopoverInput,
) {
    clear_box(content);
    content.append(
        &gtk::Label::builder()
            .label("CLI Snippets")
            .halign(gtk::Align::Start)
            .css_classes(["tile-header-popover-label"])
            .build(),
    );

    if snippets.is_empty() {
        content.append(
            &gtk::Label::builder()
                .label("No snippets configured yet. Add them in Assets.")
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["snippet-empty-state"])
                .build(),
        );
        return;
    }

    for snippet in snippets.iter().cloned() {
        let button = build_snippet_button(&snippet);
        let form_content = content.clone();
        let popover = popover.clone();
        let snippets = snippets.clone();
        let input = input.clone();
        button.connect_clicked(move |_| {
            if snippet.variables.is_empty() {
                if (input.execute)(&snippet, TemplateVariableValues::new(), &popover).is_ok() {
                    popover.popdown();
                }
            } else {
                show_snippet_variable_form(
                    &form_content,
                    &popover,
                    snippet.clone(),
                    snippets.clone(),
                    input.clone(),
                );
            }
        });
        content.append(&button);
    }
}

fn show_snippet_variable_form(
    content: &gtk::Box,
    popover: &gtk::Popover,
    snippet: CliSnippet,
    snippets: Rc<Vec<CliSnippet>>,
    input: SnippetPopoverInput,
) {
    clear_box(content);

    content.append(
        &gtk::Label::builder()
            .label(&snippet.name)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    if !snippet.description.trim().is_empty() {
        content.append(
            &gtk::Label::builder()
                .label(&snippet.description)
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["field-hint"])
                .build(),
        );
    }

    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["snippet-variable-form"])
        .build();
    let mut fields = Vec::new();
    for variable in &snippet.variables {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();
        row.append(
            &gtk::Label::builder()
                .label(&variable.label)
                .halign(gtk::Align::Start)
                .css_classes(["tile-header-popover-label"])
                .build(),
        );
        if !variable.description.trim().is_empty() {
            row.append(
                &gtk::Label::builder()
                    .label(&variable.description)
                    .halign(gtk::Align::Start)
                    .wrap(true)
                    .css_classes(["field-hint"])
                    .build(),
            );
        }
        let entry = gtk::Entry::builder()
            .hexpand(true)
            .text(&variable.default_value)
            .placeholder_text(&variable.id)
            .build();
        row.append(&entry);
        form.append(&row);
        fields.push((variable.id.clone(), entry));
    }
    content.append(&form);

    let feedback = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .visible(false)
        .css_classes(["snippet-error"])
        .build();
    content.append(&feedback);

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let back_button = icons::labeled_button("Back", icon_name::BACK, &["flat", "surface-button"]);
    back_button.set_focus_on_click(false);
    {
        let content = content.clone();
        let popover = popover.clone();
        let snippets = snippets.clone();
        let input = input.clone();
        back_button.connect_clicked(move |_| {
            refresh_snippet_list(&content, &popover, snippets.clone(), &input);
        });
    }
    actions.append(&back_button);

    let run_button = icons::labeled_button("Run", icon_name::RUN, &["flat", "surface-button"]);
    run_button.set_focus_on_click(false);
    {
        let popover = popover.clone();
        run_button.connect_clicked(move |_| {
            let variables = fields
                .iter()
                .map(|(id, entry)| (id.clone(), entry.text().to_string()))
                .collect::<TemplateVariableValues>();
            match (input.execute)(&snippet, variables, &popover) {
                Ok(()) => popover.popdown(),
                Err(error) => {
                    feedback.set_text(&error);
                    feedback.set_visible(true);
                }
            }
        });
    }
    actions.append(&run_button);
    content.append(&actions);
}

fn build_snippet_button(snippet: &CliSnippet) -> gtk::Button {
    let button = gtk::Button::builder()
        .focus_on_click(false)
        .css_classes(["flat", "snippet-list-item"])
        .build();
    let shell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .build();
    shell.append(
        &gtk::Label::builder()
            .label(&snippet.name)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["snippet-name"])
            .build(),
    );

    let mut summary_parts = Vec::new();
    if !snippet.description.trim().is_empty() {
        summary_parts.push(snippet.description.trim().to_string());
    }
    if !snippet.tags.is_empty() {
        summary_parts.push(format!("#{}", snippet.tags.join(" #")));
    }
    if !snippet.variables.is_empty() {
        summary_parts.push(format!("{} args", snippet.variables.len()));
    }
    if !summary_parts.is_empty() {
        shell.append(
            &gtk::Label::builder()
                .label(summary_parts.join("  •  "))
                .halign(gtk::Align::Start)
                .wrap(true)
                .css_classes(["snippet-description"])
                .build(),
        );
    }

    button.set_child(Some(&shell));
    button
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
