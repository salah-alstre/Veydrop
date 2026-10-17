export type PageId = "dashboard" | "connect" | "send" | "receive" | "activity" | "settings";
export interface PageProps { go: (p: PageId) => void }
