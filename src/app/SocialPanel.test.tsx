/** @vitest-environment jsdom */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ipc, type Friend, type FriendsSnapshot, type SafeAccount } from "../lib/ipc";
import { SocialPanel } from "./SocialPanel";

vi.mock("../lib/ipc", () => ({ ipc: {
  getFriends: vi.fn(), getFriendMessages: vi.fn(), sendFriendMessage: vi.fn(),
  addFriend: vi.fn(), respondToFriendRequest: vi.fn(), cancelFriendRequest: vi.fn(), removeFriend: vi.fn(),
} }));

afterEach(() => { cleanup(); delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__; });

const account: SafeAccount = { uuid: "00000000-0000-0000-0000-000000000001", username: "silas05xe", lastAuthenticatedAt: 0 };
const friend: Friend = { uuid: "00000000-0000-0000-0000-000000000002", name: "Alex", customNickname: "", note: "", online: true, lastSeen: 0, status: "Online", favorite: false, unreadMessages: 1 };
const snapshot: FriendsSnapshot = { ok: true, friends: [friend], incomingRequests: [], outgoingRequests: [] };

function renderPanel() {
  return render(<SocialPanel account={account} onSignIn={vi.fn()} />);
}

describe("friends and chat side panel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.mocked(ipc.getFriends).mockResolvedValue(snapshot);
    vi.mocked(ipc.getFriendMessages).mockResolvedValue([]);
    vi.mocked(ipc.addFriend).mockResolvedValue(snapshot);
    vi.mocked(ipc.respondToFriendRequest).mockResolvedValue(snapshot);
    vi.mocked(ipc.cancelFriendRequest).mockResolvedValue(snapshot);
    vi.mocked(ipc.removeFriend).mockResolvedValue(snapshot);
  });

  it("loads real friend state and opens a direct conversation", async () => {
    renderPanel();
    expect(await screen.findByText("Alex")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: /AlexOnline/ }));
    expect(await screen.findByText("Start a conversation")).toBeTruthy();
    expect(ipc.getFriendMessages).toHaveBeenCalledWith(friend.uuid);
  });

  it("sends a direct message through the native backend command", async () => {
    vi.mocked(ipc.sendFriendMessage).mockResolvedValue({ messageId: 42, senderUuid: account.uuid, receiverUuid: friend.uuid, senderName: account.username, message: "Hello Alex", sentAt: 1_800_000_000, read: false });
    renderPanel();
    await userEvent.click(await screen.findByRole("button", { name: /AlexOnline/ }));
    await userEvent.type(screen.getByRole("textbox", { name: "Message Alex" }), "Hello Alex");
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(await screen.findByText("Hello Alex")).toBeTruthy();
    expect(ipc.sendFriendMessage).toHaveBeenCalledWith(friend.uuid, "Hello Alex");
  });

  it("submits a friend request by username", async () => {
    renderPanel();
    await userEvent.click(screen.getByRole("button", { name: "Add friend" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Friend username" }), "Notch");
    await userEvent.click(screen.getByRole("button", { name: "Send friend request" }));
    await waitFor(() => expect(ipc.addFriend).toHaveBeenCalledWith("Notch"));
  });

  it("does not present a failed backend request as an empty friends list and offers retry", async () => {
    vi.mocked(ipc.getFriends).mockRejectedValueOnce(new Error("SNine request failed (HTTP 500)."));
    renderPanel();
    expect(await screen.findByText("Can’t connect to SNine")).toBeTruthy();
    expect(screen.queryByText("No friends yet")).toBeNull();
    expect(screen.getByText("SNine request failed (HTTP 500).")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Alex")).toBeTruthy();
  });
});
