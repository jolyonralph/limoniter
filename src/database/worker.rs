//! An open connection's own thread. Connecting and every query happen there, so a slow server or a long query
//! never holds up the window; what comes back is picked up each frame. A query is stopped from yet another thread,
//! as stopping a server's query means connecting to it again.

use super::{Connection, DbError, Reply, Schema, Stopper};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// Something for the connection to do.
pub enum Job {
    /// Run some SQL; `tag` comes back with the reply, to say what it was for.
    Run { tag: u64, sql: String, most: usize },
    /// Read what's in the database.
    Catalogue,
    /// Use another database on the same server (MySQL's USE, through COM_INIT_DB).
    UseDatabase(String),
}

/// What the connection has to say.
pub enum Event {
    Connected { server: String },
    /// Connecting failed, or the connection was lost; the thread has ended.
    Gone(DbError),
    Ran { tag: u64, result: Result<Reply, DbError> },
    Catalogue(Result<Vec<Schema>, DbError>),
    /// The database now in use, or why it couldn't be changed.
    Using(Result<String, DbError>),
}

pub struct Worker {
    jobs: Sender<Job>,
    events: Receiver<Event>,
    stopper: Arc<Mutex<Option<Stopper>>>,
}

impl Worker {
    /// Connects on a thread of its own; `wake` is called whenever there's news, to have the window look. Without a
    /// password given, the keychain's is used, looked up on that thread too, as the keychain can be slow to answer.
    pub fn start(connection: Connection, password: Option<String>, wake: impl Fn() + Send + 'static) -> Worker {
        let (jobs, job_queue) = channel::<Job>();
        let (tell, events) = channel::<Event>();
        let stopper = Arc::new(Mutex::new(None));
        let kept = stopper.clone();
        let name = format!("database {}", connection.name);
        let started = std::thread::Builder::new().name(name).spawn(move || {
            let say = |event: Event| {
                let _ = tell.send(event);
                wake();
            };
            let password = password.or_else(|| connection.engine.is_server().then(|| super::stored_password(&connection)).flatten()).unwrap_or_default();
            let mut client = match super::connect(&connection, &password) {
                Ok(client) => client,
                Err(problem) => return say(Event::Gone(problem)),
            };
            *kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(client.stopper());
            say(Event::Connected { server: client.server() });
            while let Ok(job) = job_queue.recv() {
                let lost = match job {
                    Job::Run { tag, sql, most } => {
                        let result = client.run(&sql, most);
                        let lost = result.as_ref().err().filter(|e| e.lost).cloned();
                        say(Event::Ran { tag, result });
                        lost
                    }
                    Job::UseDatabase(database) => {
                        let result = client.use_database(&database).map(|()| database);
                        let lost = result.as_ref().err().filter(|e| e.lost).cloned();
                        say(Event::Using(result));
                        lost
                    }
                    Job::Catalogue => {
                        let result = client.catalogue();
                        let lost = result.as_ref().err().filter(|e| e.lost).cloned();
                        say(Event::Catalogue(result));
                        lost
                    }
                };
                if let Some(problem) = lost {
                    return say(Event::Gone(problem));
                }
            }
        });
        if let Err(e) = started {
            let (tell, events) = channel();
            let _ = tell.send(Event::Gone(DbError::lost(e.to_string())));
            return Worker { jobs, events, stopper };
        }
        Worker { jobs, events, stopper }
    }

    pub fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    /// What's come back since last time.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Stops the query running, on a thread of its own, as that can mean connecting to the server again.
    pub fn stop_query(&self) {
        let stopper = self.stopper.clone();
        let _ = std::thread::Builder::new().name("database stop".into()).spawn(move || {
            if let Some(stopper) = stopper.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
                let _ = stopper.stop();
            }
        });
    }
}
