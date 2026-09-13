// Role: port — what the menu needs from the application shell, as opposed to from the
// session: the facts About reads, and ending the application.
//
// Separate from `BackendApi` because it is a different conversation. That port is one
// session's I/O and every call carries a session id; these two are about the window the
// session happens to be running in, and a menu that could reach the session's port would
// be able to submit commands.

export interface AboutFacts {
  name: string;
  /// What a bug report carries: `1.0.0`, or `development-521c956` (spec 26, decision 5).
  /// It is in the dialog because the dialog is copyable text; what is *read out* is the
  /// sentence below it, because an identifier is a value and not a sentence.
  version: string;
  /// The version as a listener hears it: "Version 1.0.0." or "Development build, commit
  /// 521c956."
  version_said: string;
  copyright: string;
  licence: string;
  /// Where Acter keeps everything it writes (spec 26, decision 5). A path, said as a path:
  /// the sentence around it is the dialog's, and how this copy came to be using it is the
  /// next field, because only the backend can know.
  settings_folder: string;
  /// How Acter came to be using that folder, as a whole sentence the dialog reads out
  /// after the path.
  settings_standing: string;
}

export interface AppShell {
  about(): Promise<AboutFacts>;
  /// Set the operating system's window title — what the desktop reads out in the task
  /// switcher, and what NVDA's report-title command answers.
  ///
  /// **A call rather than `document.title`**, which is what A9 tried first and what the
  /// user's NVDA disproved on 2026-08-25: assigning the document's title updates the page,
  /// and the native window keeps the title its configuration gave it.
  setTitle(title: string): Promise<void>;
  // What this window is connected to used to be asked here, and since B7 it is not: a
  // launch is no longer the only way to have a session, so the only answer that stays true
  // while the window is open is `ConnectApi.connected`'s.
  /// Which operating system this build runs on, so the frontend can decide where the
  /// menu bar belongs: in the document on Windows, in the system bar on macOS.
  platform(): Promise<string>;
  /// Ends the application, and the shell goes with it: `LocalPty::drop` kills what it
  /// spawned, so what this must not do is tear the process down around a live session
  /// (spec A7, decision 2).
  exit(): Promise<void>;
}
