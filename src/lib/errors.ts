// Shared error-envelope shape for tool IPC.
//
// Mirrors the `{ kind, message }` wire shape produced by `AppError`'s custom
// Serialize impl in `multitool-core/src/error.rs`. `kind` is the discriminant
// React components branch on; `message` is the human-readable string surfaced
// to the user. Every tool wrapper under `src/lib/tools/` rejects with this
// shape — moved out of any one tool's module so the second tool can import
// it without taking a dep on the first.

export interface AppErrorEnvelope {
  kind:
    | "FileNotFound"
    | "PermissionDenied"
    | "UnsupportedFormat"
    | "ProcessingFailed"
    | "Encrypted"
    | "Cancelled";
  message: string;
}
