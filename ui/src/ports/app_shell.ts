// Role: port — what the menu needs from the application shell, as opposed to from the
// session: the facts About reads, and ending the application.

export interface AboutFacts {
  name: string;
  version: string;
  /// The version as a listener hears it: "Version 1.0.0." or "Development build, commit
  /// 521c956."
  version_said: string;
  copyright: string;
  licence: string;
  settings_folder: string;
  /// How Acter came to be using that folder, as a whole sentence the dialog reads out
  /// after the path.
  settings_standing: string;
}

export interface AppShell {
  about(): Promise<AboutFacts>;
  /// Assigning `document.title` leaves the native window's title unchanged.
  setTitle(title: string): Promise<void>;
  platform(): Promise<string>;
  exit(): Promise<void>;
}
