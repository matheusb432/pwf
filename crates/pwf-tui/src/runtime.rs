use std::{io, time::Duration};

use anyhow::{Result, anyhow};
use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste, Event, EventStream},
    execute, queue,
    terminal::{
        BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen,
        enable_raw_mode,
    },
};
use futures::StreamExt as _;
use pwf_models::project::ProjectId;
use ratatui::{DefaultTerminal, backend::CrosstermBackend};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    app::{App, Effect, Pending},
    backend::{self, WorkerEvent},
    editor, references, view,
};

struct TerminalSession {
    terminal: DefaultTerminal,
    active: bool,
}

impl TerminalSession {
    fn new() -> Result<Self> {
        let terminal = match ratatui::try_init() {
            Ok(terminal) => terminal,
            Err(error) => {
                restore_terminal();
                return Err(error.into());
            }
        };
        let mut session = Self {
            terminal,
            active: true,
        };
        execute!(session.terminal.backend_mut(), EnableBracketedPaste)?;
        Ok(session)
    }

    fn restore(&mut self) {
        if self.active {
            restore_terminal();
            self.active = false;
        }
    }

    fn resume(&mut self) -> Result<()> {
        self.active = true;
        enable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            Clear(ClearType::All)
        )?;
        // Cursor queries after the editor would consume terminal input.
        self.terminal = DefaultTerminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(())
    }

    fn draw(&mut self, app: &mut App) -> Result<()> {
        queue!(self.terminal.backend_mut(), BeginSynchronizedUpdate)?;
        let drawn = self
            .terminal
            .draw(|frame| view::render(frame, app))
            .map(|_| ());
        let ended = execute!(self.terminal.backend_mut(), EndSynchronizedUpdate);
        drawn?;
        ended?;
        Ok(())
    }
}

fn restore_terminal() {
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        Show,
        EndSynchronizedUpdate
    );
    ratatui::restore();
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.restore();
    }
}

struct Runtime {
    app: App,
    terminal: TerminalSession,
    input: Option<EventStream>,
    events: mpsc::Sender<WorkerEvent>,
    worker: Option<JoinHandle<()>>,
    confirmation_reply: Option<oneshot::Sender<bool>>,
    request_id: u64,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.input = None;
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

pub(super) async fn run(project: Option<ProjectId>) -> Result<()> {
    let (events, mut incoming) = mpsc::channel(8);
    let mut runtime = Runtime {
        app: App::new(project),
        terminal: TerminalSession::new()?,
        input: Some(EventStream::new()),
        events,
        worker: None,
        confirmation_reply: None,
        request_id: 0,
    };
    runtime.apply(Some(runtime.app.load())).await?;
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    loop {
        runtime.terminal.draw(&mut runtime.app)?;
        let input = runtime
            .input
            .as_mut()
            .ok_or_else(|| anyhow!("Terminal input is suspended."))?;
        let effect = tokio::select! {
            event = input.next() => match event {
                Some(Ok(Event::Key(key))) => runtime.app.handle_key(key),
                Some(Ok(Event::Paste(text))) => { runtime.app.paste(&text); None }
                Some(Ok(_)) => None,
                Some(Err(error)) => return Err(error.into()),
                None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Terminal input closed.").into()),
            },
            event = incoming.recv() => runtime.worker_event(event),
            _ = tick.tick() => { runtime.app.tick = runtime.app.tick.wrapping_add(1); None },
        };
        if runtime.apply(effect).await? {
            return Ok(());
        }
    }
}

impl Runtime {
    fn worker_event(&mut self, event: Option<WorkerEvent>) -> Option<Effect> {
        let event = event?;
        let id = match &event {
            WorkerEvent::Projects { id, .. }
            | WorkerEvent::Finished { id, .. }
            | WorkerEvent::Confirm { id, .. } => *id,
        };
        if self
            .app
            .pending
            .as_ref()
            .is_none_or(|pending| pending.id != id)
        {
            return None;
        }
        match event {
            WorkerEvent::Projects { projects, .. } => {
                self.app.browser.projects = projects;
                None
            }
            WorkerEvent::Finished { id, result } => {
                self.worker = None;
                self.confirmation_reply = None;
                self.app.finished(id, result)
            }
            WorkerEvent::Confirm {
                question, reply, ..
            } => {
                self.confirmation_reply = Some(reply);
                self.app.ask_server(question);
                None
            }
        }
    }

    async fn apply(&mut self, effect: Option<Effect>) -> Result<bool> {
        let Some(effect) = effect else {
            return Ok(false);
        };
        match effect {
            Effect::Work(work) => self.start_work(work),
            Effect::CancelRead => self.cancel_read(),
            Effect::Confirm(confirmed) => {
                if let Some(reply) = self.confirmation_reply.take() {
                    let _ = reply.send(confirmed);
                }
            }
            Effect::Copy(text) => match references::copy(&text) {
                Ok(()) => self.app.notify(
                    "Sent references to the terminal clipboard (OSC 52). x exports them to a file.",
                    false,
                ),
                Err(error) => self.app.notify(format!("{error:#}"), true),
            },
            Effect::EditField(text) => {
                self.suspend();
                let result = editor::edit_input(text).await;
                self.resume()?;
                match result {
                    Ok(edited) => self.app.edited_field(&edited.text, edited.error),
                    Err(error) => self
                        .app
                        .notify(format!("{error:#} Original draft retained."), true),
                }
            }
            Effect::EditFile(path) => {
                self.suspend();
                let result = editor::edit_file(&path).await;
                self.resume()?;
                match result {
                    Ok(()) => self
                        .app
                        .notify(format!("Editor closed: {}", path.display()), false),
                    Err(error) => self.app.notify(format!("{error:#}"), true),
                }
                if let Effect::Work(work) = self.app.load() {
                    self.start_work(work);
                }
            }
            Effect::Quit => return Ok(true),
        }
        Ok(false)
    }

    fn start_work(&mut self, work: backend::Work) {
        if self.app.pending.is_some() {
            return;
        }
        self.request_id = self.request_id.wrapping_add(1);
        let id = self.request_id;
        self.app.pending = Some(Pending {
            id,
            writes: work.writes(),
        });
        self.worker = Some(tokio::spawn(backend::run(work, id, self.events.clone())));
    }

    fn cancel_read(&mut self) {
        if self
            .app
            .pending
            .as_ref()
            .is_none_or(|pending| pending.writes)
        {
            return;
        }
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
        self.app.pending = None;
        self.app.notify(
            "Read cancelled; previous records and input retained.",
            false,
        );
    }

    fn suspend(&mut self) {
        self.input = None;
        self.terminal.restore();
    }

    fn resume(&mut self) -> Result<()> {
        self.terminal.resume()?;
        self.input = Some(EventStream::new());
        Ok(())
    }
}
