use super::*;
const EXPOSURES: [&str; 4] = ["codemode", "deferred", "direct", "hidden"];
impl Resources {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context, demo: bool) {
        if self.form.open {
            self.form_dialog(ctx, demo);
        } else if self.remove.is_some() {
            self.remove_dialog(ctx);
        } else if self.review_open {
            self.review_dialog(ctx, demo);
        }
    }
    pub(super) fn edit_server(&mut self, item: &Resource) {
        self.form = Form {
            open: true,
            kind: 2,
            scope: item.scope.clone(),
            name: item.name.clone(),
            edit_id: Some(item.id.clone()),
            transport: if item.transport == "stdio" { 1 } else { 0 },
            exposure: EXPOSURES
                .iter()
                .position(|e| *e == item.exposure)
                .unwrap_or(0),
            ..Default::default()
        };
    }
    fn form_dialog(&mut self, ctx: &egui::Context, demo: bool) {
        let mut stage = false;
        let mut cancel = false;
        let trusted = self.catalog.as_ref().is_some_and(|c| c.trusted);
        let response = prompt_ui::modal(ctx, egui::Id::new("resource-add"), 570.0, |ui| {
            ui.heading(if self.form.edit_id.is_some() {
                "Edit MCP server"
            } else {
                "Add to Pi"
            });
            ui.label(theme::small(
                ui,
                "Stage a change, then review its configuration impact.",
            ));
            ui.add_space(14.0);
            egui::ScrollArea::vertical().id_salt("resource-form-body").max_height((ctx.screen_rect().height()-260.0).max(180.0)).show(ui,|ui|{
                ui.add_enabled_ui(self.form.edit_id.is_none(),|ui|{
                    ui.horizontal_wrapped(|ui|{
                        ui.label(theme::caption(ui,"SAVE TO"));
                        if theme::choice(ui,self.form.scope=="user","Personal").clicked(){self.form.scope="user".into();}
                        ui.add_enabled_ui(trusted,|ui|{if theme::choice(ui,self.form.scope=="project","Project").clicked(){self.form.scope="project".into();}});
                    });
                    ui.add_space(10.0);
                    egui::ComboBox::from_id_salt("add-resource-type").selected_text(["Pi package","Skill path","MCP server","Extension path","Prompt path","CLI theme path"][self.form.kind]).show_ui(ui,|ui|{for(i,label)in["Pi package","Skill path","MCP server","Extension path","Prompt path","CLI theme path"].iter().enumerate(){ui.selectable_value(&mut self.form.kind,i,*label);}});
                });
                ui.add_space(12.0);
                if self.form.kind==2 {
                    ui.label(theme::caption(ui,"SERVER NAME"));
                    ui.add_enabled(self.form.edit_id.is_none(),theme::text_field(&mut self.form.name,"documentation").desired_width(ui.available_width()));
                    ui.add_space(8.0);
                    ui.horizontal(|ui|{if theme::choice(ui,self.form.transport==0,"HTTP").clicked(){self.form.transport=0;}
if theme::choice(ui,self.form.transport==1,"stdio").clicked(){self.form.transport=1;}});
                    if self.form.transport==0{
                        ui.label(theme::caption(ui,"STREAMABLE HTTP URL"));ui.add(theme::text_field(&mut self.form.url,"https://example.com/mcp").desired_width(ui.available_width()));
                    }else{
                        ui.label(theme::caption(ui,"EXECUTABLE · NOT A SHELL COMMAND"));ui.add(theme::text_field(&mut self.form.command,"npx or /absolute/path/to/server").desired_width(ui.available_width()));
                        ui.label(theme::caption(ui,"ARGUMENTS · JSON ARRAY"));ui.add(theme::text_field(&mut self.form.args,"[\"--flag\", \"value\"]").desired_width(ui.available_width()));
                    }
                    ui.add_space(8.0);
                    ui.label(theme::caption(ui,"TOOL EXPOSURE"));egui::ComboBox::from_id_salt("mcp-exposure").selected_text(EXPOSURES[self.form.exposure]).show_ui(ui,|ui|{for(i,label)in EXPOSURES.iter().enumerate(){ui.selectable_value(&mut self.form.exposure,i,*label);}});
                    ui.collapsing("Advanced JSON patch",|ui|{ui.add(egui::TextEdit::multiline(&mut self.form.advanced).code_editor().desired_width(f32::INFINITY).desired_rows(4).hint_text("{\"headers\":{\"Authorization\":\"Bearer ${DOCS_TOKEN}\"}}"));});
                    ui.add(egui::Label::new(theme::small(ui,"Omitted fields, including credentials, are preserved. Enter environment references rather than literal secrets. Open config to inspect or remove existing fields.")).wrap());
                }else{
                    ui.label(theme::caption(ui,if self.form.kind==0{"PACKAGE SOURCE"}else{"LOCAL PATH"}));
                    ui.add(theme::text_field(&mut self.form.source,if self.form.kind==0{"npm:@scope/name@version, git:… or /local/package"}else{"Absolute or workspace-relative file / directory"}).desired_width(ui.available_width()));
                    ui.add(egui::Label::new(theme::small(ui,if self.form.kind==0{"No marketplace lookup: remote contents are unknown until installed. Use an exact npm version or Git ref when possible."}else{"The path is registered in Pi. Files stay in place; no copy or deletion."})).wrap());
                }
                ui.add_space(12.0);ui.add(egui::Label::new(RichText::new("Packages may execute dependency code. Extensions and stdio MCP servers run with your OS permissions when loaded. Browsing does not execute or connect them.").size(11.0).color(theme::palette(ctx).amber)).wrap());
                if let Some(error)=&self.form.error{ui.add(egui::Label::new(RichText::new(error).color(theme::palette(ctx).red).size(12.0)).wrap());}
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                cancel = ui.button("Cancel").clicked();
                stage = ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::new("Stage & review →").fill(theme::palette(ctx).accent_bg),
                    )
                    .clicked();
            });
        });
        if cancel || response.should_close() {
            self.form.open = false;
        }
        if stage {
            match self.form_request() {
                Ok(request) => {
                    let title = if self.form.kind == 2 {
                        self.form.name.clone()
                    } else {
                        self.form.source.clone()
                    };
                    let key = if let Some(id) = &self.form.edit_id {
                        format!("mcp-edit:{id}")
                    } else {
                        format!("add:{}:{}", self.form.kind, title)
                    };
                    self.stage(key, title, request);
                    self.form.open = false;
                    self.request_review(ctx, demo);
                }
                Err(error) => self.form.error = Some(error),
            }
        }
    }
    pub(super) fn form_request(&self) -> Result<Value, String> {
        let form = &self.form;
        if form.kind != 2 {
            if form.source.trim().is_empty() {
                return Err("Enter a package source or local path.".into());
            }
            return Ok(
                json!({"action":if form.kind==0{"install"}else{"register"},"kind":match form.kind{1=>"skills",3=>"extensions",4=>"prompts",5=>"themes",_=>"packages"},"scope":form.scope,"source":form.source.trim()}),
            );
        }
        if form.name.is_empty()
            || !form
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("Server names use letters, digits, underscores or hyphens.".into());
        }
        let mut definition = json!({"exposure":EXPOSURES[form.exposure]});
        if form.transport == 0 && !form.url.trim().is_empty() {
            if !form.url.starts_with("https://") && !form.url.starts_with("http://") {
                return Err("Enter a valid HTTP or HTTPS endpoint.".into());
            }
            definition["url"] = form.url.trim().into();
        }
        if form.transport == 1 && !form.command.trim().is_empty() {
            definition["command"] = form.command.trim().into();
        }
        if form.transport == 1 && !form.args.trim().is_empty() {
            let args: Value = serde_json::from_str(&form.args)
                .map_err(|_| "Arguments must be a JSON array of strings.")?;
            if !args
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string))
            {
                return Err("Arguments must be a JSON array of strings.".into());
            }
            definition["args"] = args;
        }
        if !form.advanced.trim().is_empty() {
            let patch: Value = serde_json::from_str(&form.advanced)
                .map_err(|_| "Advanced patch must be valid JSON.")?;
            let object = patch
                .as_object()
                .ok_or("Advanced patch must be an object.")?;
            for (k, v) in object {
                definition[k] = v.clone();
            }
        }
        if form.edit_id.is_none() && definition["url"].is_null() && definition["command"].is_null()
        {
            return Err("Enter a server URL or executable.".into());
        }
        Ok(
            json!({"action":if form.edit_id.is_some(){"mcp-edit"}else{"mcp-add"},"kind":"mcp","id":form.edit_id,"scope":form.scope,"name":form.name,"definition":serde_json::to_string(&definition).unwrap_or_default()}),
        )
    }
    fn remove_dialog(&mut self, ctx: &egui::Context) {
        let item = self.remove.as_ref().unwrap().clone();
        let mut stage = false;
        let mut cancel = false;
        let response = prompt_ui::modal(ctx, egui::Id::new("resource-remove"), 540.0, |ui| {
            ui.heading(if item.kind == "packages" {
                "Uninstall package?"
            } else if item.kind == "mcp" {
                "Remove server entry?"
            } else {
                "Exclude resource?"
            });
            ui.label(RichText::new(&item.name).strong());
            ui.add_space(12.0);
            ui.add(egui::Label::new(theme::small(ui,match item.kind.as_str(){"packages"=>"Removes the managed installation and declaration. Local package source files stay intact. These resolved resources belong to this package:","mcp"=>"Removes this entry. OAuth credentials stay intact. Removing a project entry can reveal a personal server with the same name.",_=>"Adds a Pi exclusion filter; it does not delete source files. Uninstall the owning package separately to remove its managed copy."})).wrap());
            if item.kind == "packages" {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        if let Some(c) = &self.catalog {
                            for r in &c.items {
                                if r.kind != "packages"
                                    && !item.owner.is_empty()
                                    && r.owner == item.owner
                                {
                                    ui.label(theme::small(
                                        ui,
                                        format!("{} · {}", r.name, r.kind_label()),
                                    ));
                                }
                            }
                        }
                    });
            }
            ui.add_space(12.0);
            ui.label(theme::small(
                ui,
                "This stages a change. Nothing happens until the review is confirmed.",
            ));
            ui.horizontal(|ui| {
                cancel = ui.button("Cancel").clicked();
                stage = ui
                    .button(RichText::new("Stage removal").color(theme::palette(ctx).red))
                    .clicked();
            });
        });
        if cancel || response.should_close() {
            self.remove = None;
        }
        if stage {
            self.stage(
                format!("remove:{}", item.id),
                format!("Remove {}", item.name),
                json!({"action":"remove","id":item.id,"scope":item.scope}),
            );
            self.remove = None;
        }
    }
    fn review_dialog(&mut self, ctx: &egui::Context, demo: bool) {
        let review = self.review.clone();
        let mut apply = false;
        let mut close = false;
        let mut remove = None;
        let response = prompt_ui::modal(ctx, egui::Id::new("resource-review"), 690.0, |ui| {
            ui.heading("Review changes");
            ui.label(theme::small(
                ui,
                format!(
                    "{} operations · revalidated before applying",
                    self.drafts.len()
                ),
            ));
            ui.add_space(12.0);
            egui::ScrollArea::vertical().id_salt("resource-review-body").max_height((ctx.screen_rect().height()-280.0).max(180.0)).show(ui,|ui|{
                if let Some(review)=&review {for step in &review.steps{
                    ui.horizontal_wrapped(|ui|{ui.label(RichText::new(&step.title).strong());if ui.small_button("Remove draft").clicked(){remove=Some(step.index);}});
                    ui.add(egui::Label::new(RichText::new(&step.file).monospace().size(10.5).color(theme::palette(ctx).muted)).wrap().selectable(true));
                    if !step.operation.is_empty(){ui.add(egui::Label::new(theme::small(ui,&step.operation)).wrap());}
                    else {
                        if step.redacted { ui.label(RichText::new("Sensitive MCP fields redacted. Unchanged credentials are preserved.").size(10.5).color(theme::palette(ctx).amber)); }
                        if !step.diff.is_empty() { markdown::diff(ui, &step.diff); }
                        else {
                            ui.label(theme::caption(ui,"BEFORE"));ui.add(egui::Label::new(RichText::new(&step.before).monospace().size(10.5)).wrap().selectable(true));
                            ui.label(theme::caption(ui,"AFTER"));ui.add(egui::Label::new(RichText::new(&step.after).monospace().size(10.5).color(theme::palette(ctx).accent)).wrap().selectable(true));
                        }
                    }
                    for name in &step.affected{ui.label(theme::small(ui,format!("Includes: {name}")));}ui.add_space(8.0);ui.separator();ui.add_space(8.0);
                }}else if self.job.is_some(){ui.label("Preparing review…");}else{ui.label(theme::small(ui,"Review is unavailable or expired. Close and review drafts again."));}
                for outcome in &self.outcomes{ui.add(egui::Label::new(RichText::new(format!("Operation {}: {}",outcome.index+1,outcome.message)).size(11.0).color(if outcome.success{theme::palette(ctx).accent}else{theme::palette(ctx).red})).wrap());}
                ui.add(egui::Label::new(theme::small(ui,"Operations run sequentially, not as an atomic transaction. On failure, earlier operations may remain applied; remaining changes are kept for explicit review. Package scripts cannot be rolled back. Existing agents are not reloaded.")).wrap());
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                close = ui.button("Back").clicked();
                apply = ui
                    .add_enabled(
                        review.is_some() && self.job.is_none(),
                        egui::Button::new(if demo {
                            "Simulate apply"
                        } else {
                            "Apply reviewed changes"
                        })
                        .fill(theme::palette(ctx).accent_bg),
                    )
                    .clicked();
            });
        });
        if close || response.should_close() {
            self.review_open = false;
        }
        if let Some(index) = remove {
            if index < self.drafts.len() {
                self.drafts.remove(index);
                self.review = None;
                self.review_open = false;
            }
        }
        if apply {
            if demo {
                self.simulate_apply();
                self.review_open = false;
            } else if let Some(review) = review {
                self.start(ctx,JobKind::Apply,json!({"action":"apply","revision":self.catalog.as_ref().map(|c|c.revision.clone()).unwrap_or_default(),"token":review.token,"operations":self.drafts.iter().map(|d|&d.request).collect::<Vec<_>>() }));
            }
        }
    }
    fn simulate_apply(&mut self) {
        if let Some(catalog) = &mut self.catalog {
            for draft in &self.drafts {
                if let Some(item) = catalog
                    .items
                    .iter_mut()
                    .find(|r| draft.request["id"] == r.id)
                {
                    if draft.request["action"] == "select" {
                        if draft.request["scope"] == "user" {
                            item.personal_enabled = Some(draft.request["state"] == "on");
                        } else {
                            item.project_state =
                                draft.request["state"].as_str().unwrap_or("inherit").into();
                        }
                    }
                }
            }
        }
        self.drafts.clear();
        self.review = None;
        self.notice = Some(
            "Demo simulation complete. No files, packages or running agents were changed.".into(),
        );
    }
}
