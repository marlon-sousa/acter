// Role: port (driving) — what the frontend may ask of the backend.

export interface BackendApi {
  echo(text: string): Promise<string>;
}
