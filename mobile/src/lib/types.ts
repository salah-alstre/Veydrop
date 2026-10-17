// Wire types for the phone API served by the desktop app. All camelCase.

export type TransferState =
  | "pending" | "queued" | "active" | "paused" | "interrupted"
  | "finalizing" | "completed" | "failed" | "cancelled" | "rejected";

export type OfferState = "pending" | "accepted" | "rejected" | "failed";

export interface OfferFile {
  clientId: string;
  name: string;
  relPath: string | null;
  size: number;
}

export interface UploadRef { clientId: string; transferId: string }

export interface PhoneOffer {
  id: string;
  state: OfferState;
  error: string | null;
  uploads: UploadRef[];
}

export interface UploadStatus {
  transferId: string;
  received: number;
  size: number;
  state: TransferState;
  error: string | null;
}

export interface PhoneUpload {
  transferId: string;
  clientId: string;
  offerId: string;
  name: string;
  size: number;
  transferred: number;
  state: TransferState;
  error: string | null;
}

export interface PhoneSnapshot { offers: PhoneOffer[]; uploads: PhoneUpload[] }

export interface SessionInfo {
  id: string;
  deviceLabel: string;
  platform: string;
  remoteIp: string;
  connectedAt: number;
  lastSeen: number;
  expiresAt: number | null;
  online: boolean;
  trusted: boolean;
}

export interface ShareItemInfo {
  id: string;
  name: string;
  relPath: string;
  size: number;
  sha256: string | null;
  ready: boolean;
  missing: boolean;
}

export interface ShareInfo {
  id: string;
  createdAt: number;
  active: boolean;
  items: ShareItemInfo[];
  totalSize: number;
  indexed: boolean;
}

export type SessionResponse =
  | { paired: false }
  | { paired: true; session: SessionInfo; pcName: string; language?: string };

export interface PairRequestResponse {
  id: string;
  code: string;
  claim: string;
  expiresAt: number;
  trusted: boolean;
  pcName: string;
  language?: string;
}

export type ClaimResponse =
  | { status: "pending" }
  | { status: "rejected" }
  | { status: "expired" }
  | { status: "approved"; session: SessionInfo; pcName: string; language?: string };

export interface CompleteResponse {
  transferId: string;
  state: TransferState;
  transferred: number;
  size: number;
  error: string | null;
  sha256: string | null;
}

export interface HelloEvent {
  session: SessionInfo;
  pcName: string;
  language?: string;
  share: ShareInfo | null;
  transfers: PhoneSnapshot;
}
