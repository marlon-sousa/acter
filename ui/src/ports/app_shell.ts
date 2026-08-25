// Role: port — what the menu needs from the application shell, as opposed to from the
// session: the facts About reads, and ending the application.
//
// Separate from `BackendApi` because it is a different conversation. That port is one
// session's I/O and every call carries a session id; these two are about the window the
// session happens to be running in, and a menu that could reach the session's port would
// be able to submit commands.

export interface AboutFacts {
  name: string;
  version: string;
  copyright: string;
  licence: string;
}

export interface AppShell {
  about(): Promise<AboutFacts>;
  /// What this window is connected to, or `null` for the scripted session — which nobody
  /// chose and which has no name worth putting in a title bar (spec A9).
  connection(): Promise<string | null>;
  /// Which operating system this build runs on, so the frontend can decide where the
  /// menu bar belongs: in the document on Windows, in the system bar on macOS.
  platform(): Promise<string>;
  /// Ends the application, and the shell goes with it: `LocalPty::drop` kills what it
  /// spawned, so what this must not do is tear the process down around a live session
  /// (spec A7, decision 2).
  exit(): Promise<void>;
}
