import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ClipboardEvent, type FormEvent } from "react";
import { ArrowDown, ArrowLeft, Check, Clock3, Copy, ImagePlus, LoaderCircle, MessageSquare, Plus, Search, Send, Trash2, Users, X } from "lucide-react";
import type { ActivityEntry, Friend, FriendMessage, FriendsSnapshot, Profile, SafeAccount } from "../lib/ipc";
import { ipc } from "../lib/ipc";
import { MinecraftHead } from "./MinecraftHead";

export type SocialTab = "Friends" | "Chats";

type Props = {
  tab: SocialTab;
  onTabChange: (tab: SocialTab) => void;
  account: SafeAccount | null;
  activity: ActivityEntry[];
  profiles: Profile[];
  onSelectProfile: (profile: Profile) => void;
  onSignIn: () => void;
};

const emptySnapshot: FriendsSnapshot = { ok: false, friends: [], incomingRequests: [], outgoingRequests: [] };
const GIPHY_API_KEY = import.meta.env.VITE_GIPHY_API_KEY?.trim() ?? "";
type GiphyGif = { id: string; title: string; images?: { fixed_width?: { url?: string }; downsized?: { url?: string }; original?: { url?: string } }; analytics?: { onsent?: { url?: string } } };

export function SocialPanel({ tab, onTabChange, account, activity, profiles, onSelectProfile, onSignIn }: Props) {
  const [snapshot, setSnapshot] = useState<FriendsSnapshot>(emptySnapshot);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [friendNotice, setFriendNotice] = useState("");
  const [chatLoadError, setChatLoadError] = useState("");
  const [chatActionError, setChatActionError] = useState("");
  const [chatCopyNotice, setChatCopyNotice] = useState("");
  const [query, setQuery] = useState("");
  const [addingFriend, setAddingFriend] = useState(false);
  const [friendName, setFriendName] = useState("");
  const [busy, setBusy] = useState(false);
  const [selectedFriendId, setSelectedFriendId] = useState<string | null>(null);
  const [messages, setMessages] = useState<FriendMessage[]>([]);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [draftsByFriend, setDraftsByFriend] = useState<Record<string, string>>({});
  const [sendingFriendUuid, setSendingFriendUuid] = useState<string | null>(null);
  const [gifComposerOpen, setGifComposerOpen] = useState(false);
  const [gifUrl, setGifUrl] = useState("");
  const [gifSearch, setGifSearch] = useState("");
  const [gifResults, setGifResults] = useState<GiphyGif[]>([]);
  const [gifLoading, setGifLoading] = useState(false);
  const [gifError, setGifError] = useState("");
  const chatMessagesRef = useRef<HTMLDivElement>(null);
  const chatSendLockRef = useRef(false);
  const selectedFriendIdRef = useRef<string | null>(null);
  const stickToLatestRef = useRef(true);
  const [showLatestMessageButton, setShowLatestMessageButton] = useState(false);

  const refreshFriends = useCallback(async (showLoading = false) => {
    if (!account || !("__TAURI_INTERNALS__" in window)) return;
    if (showLoading) setLoading(true);
    try {
      const next = await ipc.getFriends();
      setSnapshot(next);
      setError("");
    } catch (reason) {
      setError(message(reason));
    } finally {
      setLoading(false);
    }
  }, [account]);

  useEffect(() => {
    setSnapshot(emptySnapshot);
    setError("");
    setFriendNotice("");
    setMessages([]);
    setMessagesLoading(false);
    setDraftsByFriend({});
    setChatLoadError("");
    setChatActionError("");
    setChatCopyNotice("");
    setGifComposerOpen(false);
    setGifUrl("");
    setGifSearch("");
    selectedFriendIdRef.current = null;
    setSelectedFriendId(null);
    if (!account) {
      setLoading(false);
      return;
    }
    let active = true;
    const refresh = async () => {
      try {
        const next = await ipc.getFriends();
        if (active) { setSnapshot(next); setError(""); }
      } catch (reason) {
        if (active) setError(message(reason));
      } finally {
        if (active) setLoading(false);
      }
    };
    setLoading(true);
    void refresh();
    const timer = window.setInterval(() => void refresh(), 20000);
    return () => { active = false; window.clearInterval(timer); };
  }, [account?.uuid]);

  useEffect(() => {
    if (!friendNotice) return;
    const timer = window.setTimeout(() => setFriendNotice(""), 4000);
    return () => window.clearTimeout(timer);
  }, [friendNotice]);

  useEffect(() => {
    if (!chatCopyNotice) return;
    const timer = window.setTimeout(() => setChatCopyNotice(""), 2200);
    return () => window.clearTimeout(timer);
  }, [chatCopyNotice]);

  const selectedFriend = useMemo(
    () => snapshot.friends.find(friend => friend.uuid === selectedFriendId) ?? null,
    [snapshot.friends, selectedFriendId],
  );
  const draft = selectedFriend ? draftsByFriend[selectedFriend.uuid] ?? "" : "";
  const setDraft = (value: string) => {
    if (!selectedFriend) return;
    const uuid = selectedFriend.uuid;
    setDraftsByFriend(current => ({ ...current, [uuid]: value }));
  };

  useEffect(() => {
    if (tab !== "Chats" || !selectedFriend || !("__TAURI_INTERNALS__" in window)) {
      setMessages([]);
      setMessagesLoading(false);
      setChatLoadError("");
      return;
    }
    let active = true;
    let requestInFlight = false;
    const refresh = async () => {
      if (requestInFlight) return;
      requestInFlight = true;
      try {
        const next = await ipc.getFriendMessages(selectedFriend.uuid);
        if (active && selectedFriendIdRef.current === selectedFriend.uuid) { setMessages(next); setChatLoadError(""); }
      } catch (reason) {
        if (active && selectedFriendIdRef.current === selectedFriend.uuid) setChatLoadError(message(reason));
      } finally {
        requestInFlight = false;
        if (active && selectedFriendIdRef.current === selectedFriend.uuid) setMessagesLoading(false);
      }
    };
    setMessagesLoading(true);
    void refresh();
    const timer = window.setInterval(() => void refresh(), 8000);
    return () => { active = false; window.clearInterval(timer); };
  }, [tab, selectedFriend?.uuid]);

  useLayoutEffect(() => {
    const container = chatMessagesRef.current;
    if (!container || !selectedFriend) return;
    if (stickToLatestRef.current) {
      container.scrollTop = container.scrollHeight;
      setShowLatestMessageButton(false);
    } else {
      setShowLatestMessageButton(true);
    }
  }, [messages, selectedFriend?.uuid]);

  useEffect(() => {
    const container = chatMessagesRef.current;
    if (!container || !selectedFriend || tab !== "Chats" || typeof ResizeObserver === "undefined") return;
    const keepAtLatest = () => {
      if (stickToLatestRef.current) container.scrollTop = container.scrollHeight;
    };
    const observer = new ResizeObserver(keepAtLatest);
    observer.observe(container);
    for (const child of Array.from(container.children)) {
      if (child instanceof HTMLElement && child.classList.contains("chat-message")) observer.observe(child);
    }
    window.addEventListener("resize", keepAtLatest);
    return () => { observer.disconnect(); window.removeEventListener("resize", keepAtLatest); };
  }, [messages, selectedFriend?.uuid, tab]);

  useEffect(() => {
    if (!gifComposerOpen) { setGifResults([]); setGifLoading(false); setGifError(""); return; }
    if (!GIPHY_API_KEY) { setGifResults([]); setGifLoading(false); setGifError("GIF search is not configured for this launcher build yet."); return; }
    const controller = new AbortController();
    // Hide previous results immediately so they cannot be sent under a newer query.
    setGifResults([]);
    setGifLoading(true);
    setGifError("");
    const timer = window.setTimeout(() => {
      setGifError("");
      const endpoint = gifSearch.trim() ? "search" : "trending";
      const params = new URLSearchParams({ api_key: GIPHY_API_KEY, limit: "24", rating: "pg", lang: "en" });
      if (endpoint === "search") params.set("q", gifSearch.trim().slice(0, 50));
      void fetch(`https://api.giphy.com/v1/gifs/${endpoint}?${params}`, { signal: controller.signal })
        .then(async response => {
          if (!response.ok) throw new Error(response.status === 401 || response.status === 403 ? "GIPHY rejected this API key. Check it and try again." : response.status === 429 ? "GIPHY search is rate limited. Try again in a little while." : `GIPHY search failed (${response.status}).`);
          return response.json() as Promise<{ data?: GiphyGif[] }>;
        })
        .then(payload => { if (!controller.signal.aborted) setGifResults(Array.isArray(payload.data) ? payload.data : []); })
        .catch(reason => { if (!controller.signal.aborted) { setGifResults([]); setGifError(reason instanceof Error ? reason.message : "Could not load GIFs."); } })
        .finally(() => { if (!controller.signal.aborted) setGifLoading(false); });
    }, gifSearch.trim() ? 320 : 80);
    return () => { window.clearTimeout(timer); controller.abort(); };
  }, [gifComposerOpen, gifSearch]);

  useEffect(() => {
    if (!gifComposerOpen) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setGifComposerOpen(false);
      setGifUrl("");
      setGifSearch("");
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [gifComposerOpen]);

  const filteredFriends = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return [...snapshot.friends]
      .sort((left, right) => Number(right.favorite) - Number(left.favorite) || Number(right.online) - Number(left.online) || displayName(left).localeCompare(displayName(right)))
      .filter(friend => !normalized || `${displayName(friend)} ${friend.name} ${friend.status}`.toLocaleLowerCase().includes(normalized));
  }, [query, snapshot.friends]);

  const runFriendAction = async (operation: () => Promise<FriendsSnapshot>, success: string) => {
    setBusy(true);
    setError("");
    setFriendNotice("");
    try {
      setSnapshot(await operation());
      setFriendName("");
      setAddingFriend(false);
      setError("");
      if (success) setFriendNotice(success);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  };

  const sendRequest = (event: FormEvent) => {
    event.preventDefault();
    const name = friendName.trim();
    if (name) void runFriendAction(() => ipc.addFriend(name), "Friend request sent.");
  };

  const answerRequest = (uuid: string, accept: boolean) => {
    void runFriendAction(() => ipc.respondToFriendRequest(uuid, accept), accept ? "Friend request accepted." : "Friend request declined.");
  };

  const removeFriend = (friend: Friend) => {
    if (!window.confirm(`Remove ${displayName(friend)} from your friends?`)) return;
    void runFriendAction(() => ipc.removeFriend(friend.uuid), "Friend removed.");
    if (selectedFriendId === friend.uuid) { selectedFriendIdRef.current = null; setSelectedFriendId(null); setMessages([]); }
  };

  const openChat = (friend: Friend) => {
    selectedFriendIdRef.current = friend.uuid;
    setSelectedFriendId(friend.uuid);
    setMessages([]);
    stickToLatestRef.current = true;
    setShowLatestMessageButton(false);
    setError("");
    setChatLoadError("");
    setChatActionError("");
    onTabChange("Chats");
  };

  const closeChat = () => {
    selectedFriendIdRef.current = null;
    setSelectedFriendId(null);
    setMessages([]);
    stickToLatestRef.current = true;
    setShowLatestMessageButton(false);
    setChatLoadError("");
    setChatActionError("");
  };

  const onChatScroll = () => {
    const container = chatMessagesRef.current;
    if (!container) return;
    const isAtLatest = container.scrollHeight - container.scrollTop - container.clientHeight < 56;
    stickToLatestRef.current = isAtLatest;
    setShowLatestMessageButton(!isAtLatest);
  };

  const jumpToLatest = () => {
    const container = chatMessagesRef.current;
    if (!container) return;
    stickToLatestRef.current = true;
    setShowLatestMessageButton(false);
    container.scrollTo({ top: container.scrollHeight, behavior: "smooth" });
  };

  const handleMessagePaste = (event: ClipboardEvent<HTMLInputElement>) => {
    const pastedText = event.clipboardData.getData("text").trim();
    if (!isDirectGifUrl(pastedText)) return;
    event.preventDefault();
    setGifUrl(pastedText);
    setGifComposerOpen(true);
    setChatActionError("");
  };

  const copyMessage = async (content: string) => {
    const text = content.startsWith("SNINEGIF:") ? content.slice("SNINEGIF:".length) : content;
    try { await navigator.clipboard.writeText(text); setChatCopyNotice("Message copied."); }
    catch { setChatActionError("Clipboard access is unavailable."); }
  };

  const sendMessage = async (event: FormEvent) => {
    event.preventDefault();
    if (!selectedFriend || !draft.trim() || busy || chatSendLockRef.current) return;
    const friendUuid = selectedFriend.uuid;
    const messageText = draft.trim();
    chatSendLockRef.current = true;
    stickToLatestRef.current = true;
    setSendingFriendUuid(friendUuid);
    setBusy(true);
    setChatActionError("");
    try {
      const sent = await ipc.sendFriendMessage(friendUuid, messageText);
      if (selectedFriendIdRef.current === friendUuid) setMessages(current => [...current.filter(item => item.messageId !== sent.messageId), sent]);
      setDraftsByFriend(current => current[friendUuid] === messageText ? { ...current, [friendUuid]: "" } : current);
    } catch (reason) {
      if (selectedFriendIdRef.current === friendUuid) setChatActionError(message(reason));
    } finally {
      chatSendLockRef.current = false;
      setSendingFriendUuid(current => current === friendUuid ? null : current);
      setBusy(false);
    }
  };

  const deliverGif = async (normalized: string, analyticsUrl?: string) => {
    if (!selectedFriend || busy || chatSendLockRef.current) return;
    if (!isDirectGifUrl(normalized)) {
      setChatActionError("Use a direct HTTPS GIF from GIPHY or Tenor.");
      return;
    }
    const payload = `SNINEGIF:${new URL(normalized).toString()}`;
    if (payload.length > 240) { setChatActionError("That GIF link is too long. Use a shorter direct GIF link."); return; }
    const friendUuid = selectedFriend.uuid;
    chatSendLockRef.current = true;
    stickToLatestRef.current = true;
    setSendingFriendUuid(friendUuid);
    setBusy(true);
    setChatActionError("");
    try {
      const sent = await ipc.sendFriendMessage(friendUuid, payload);
      if (selectedFriendIdRef.current === friendUuid) {
        setMessages(current => [...current.filter(item => item.messageId !== sent.messageId), sent]);
        setGifUrl("");
        setGifComposerOpen(false);
        setGifSearch("");
      }
      if (analyticsUrl) void registerGifSend(analyticsUrl);
    } catch (reason) { if (selectedFriendIdRef.current === friendUuid) setChatActionError(message(reason)); }
    finally { chatSendLockRef.current = false; setSendingFriendUuid(current => current === friendUuid ? null : current); setBusy(false); }
  };

  const sendGif = (event: FormEvent) => { event.preventDefault(); void deliverGif(gifUrl.trim()); };

  const renderChatMessage = (content: string) => {
    const gif = content.startsWith("SNINEGIF:") ? content.slice("SNINEGIF:".length) : null;
    if (!gif) return <p>{content}</p>;
    if (!isDirectGifUrl(gif)) return <p>GIF · Only supported in the SNine Launcher</p>;
    return <div className="chat-gif"><img src={gif} alt="Animated GIF" loading="lazy" referrerPolicy="no-referrer" /><small>GIF · visible in SNine Launcher</small></div>;
  };

  return <>
    <div className="side-tabs social-tabs" role="tablist" aria-label="Social">
      <button role="tab" aria-selected={tab === "Friends"} className={tab === "Friends" ? "active" : ""} onClick={() => onTabChange("Friends")}><Users size={16} /> Friends</button>
      <button role="tab" aria-selected={tab === "Chats"} className={tab === "Chats" ? "active" : ""} onClick={() => onTabChange("Chats")}><MessageSquare size={15} /> Chats{snapshot.friends.some(friend => friend.unreadMessages > 0) && <i className="social-unread-dot" />}</button>
    </div>
    {!account ? <div className="side-content social-content"><div className="social-empty"><div className="activity-symbol"><Users size={17} /></div><strong>Sign in to SNine</strong><p>Your friends and chats are connected to your Minecraft account.</p><button className="secondary-action" onClick={onSignIn}>Sign in</button></div></div>
      : tab === "Friends" ? <div className="side-content social-content">
        <div className="social-account-row"><Avatar name={account.username} uuid={account.uuid} /><span><strong>{account.username}</strong><small><i className={`presence-dot ${snapshot.ok && !error ? "online" : ""}`} /> {loading ? "Connecting…" : snapshot.ok && !error ? "Online · SNine Launcher" : error ? "Disconnected" : "Offline"}</small></span><button className="social-refresh" title="Refresh friends" aria-label="Refresh friends" onClick={() => { setLoading(true); void refreshFriends(true); }}><Clock3 size={14} /></button></div>
        <div className="social-group-row"><strong>Friends</strong><span>{snapshot.friends.length}</span><button className="social-icon-button" title="Add friend" aria-label="Add friend" onClick={() => setAddingFriend(value => !value)}><Plus size={16} /></button><button className="social-icon-button" title="Search friends" aria-label="Search friends" onClick={() => document.getElementById("social-friend-search")?.focus()}><Search size={15} /></button></div>
        {addingFriend && <form className="friend-add-form" onSubmit={sendRequest}><input autoFocus aria-label="Friend username" placeholder="Minecraft or SNine name" maxLength={32} value={friendName} onChange={event => setFriendName(event.target.value)} /><button aria-label="Send friend request" disabled={busy || !friendName.trim()}><Plus size={15} /></button><button type="button" aria-label="Cancel friend request" onClick={() => { setAddingFriend(false); setFriendName(""); }}><X size={15} /></button></form>}
        <label className="social-search"><Search size={14} /><input id="social-friend-search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Search friends" aria-label="Search friends" />{query && <button type="button" aria-label="Clear friend search" onClick={() => setQuery("")}><X size={13} /></button>}</label>
        {snapshot.incomingRequests.length > 0 && <section className="friend-requests"><h3>Friend requests <span>{snapshot.incomingRequests.length}</span></h3>{snapshot.incomingRequests.map(request => <article className="friend-request-row" key={request.uuid}><Avatar name={request.name} uuid={request.uuid} /><span><strong>{request.name}</strong><small>Wants to be your friend</small></span><button title="Accept request" aria-label={`Accept ${request.name}`} disabled={busy} onClick={() => answerRequest(request.uuid, true)}><Check size={14} /></button><button title="Decline request" aria-label={`Decline ${request.name}`} disabled={busy} onClick={() => answerRequest(request.uuid, false)}><X size={14} /></button></article>)}</section>}
        {snapshot.outgoingRequests.length > 0 && <section className="outgoing-requests"><h3>Sent requests</h3>{snapshot.outgoingRequests.map(request => <div key={request.uuid}><span>{request.name}</span><small>Pending</small></div>)}</section>}
        {friendNotice && <p className="social-status-message success" role="status">{friendNotice}</p>}{error && snapshot.ok && <p className="social-status-message error" role="alert">{error}</p>}
        {loading && snapshot.friends.length === 0 ? <div className="social-loading">Loading friends…</div> : error && !snapshot.ok ? <div className="social-empty compact"><Users size={18} /><strong>Can’t connect to SNine</strong><p>{error}</p><button className="secondary-action" onClick={() => { setLoading(true); void refreshFriends(true); }}>Retry</button></div> : filteredFriends.length === 0 ? <div className="social-empty compact"><Users size={18} /><strong>{snapshot.friends.length ? "No friends match" : "No friends yet"}</strong><p>{snapshot.friends.length ? "Try another search." : "Add a friend by their Minecraft username."}</p></div> : <div className="friend-list">{filteredFriends.map(friend => <FriendRow key={friend.uuid} friend={friend} onChat={() => openChat(friend)} onRemove={() => removeFriend(friend)} />)}</div>}
        <section className="social-activity"><h3>Recent activity</h3>{activity.length === 0 ? <small>No recent activity</small> : activity.slice(0, 3).map(entry => { const profile = profiles.find(item => item.id === entry.profileId); return <button key={entry.id} disabled={!profile} onClick={() => profile && onSelectProfile(profile)}><i /><span><strong>{entry.title}</strong><small>{entry.detail} · {formatAge(entry.occurredAt)}</small></span></button>; })}</section>
      </div> : <div className="side-content social-content">
        {selectedFriend ? <><div className="chat-header"><button className="social-icon-button" aria-label="Back to conversations" onClick={closeChat}><ArrowLeft size={16} /></button><Avatar name={displayName(selectedFriend)} uuid={selectedFriend.uuid} /><span><strong>{displayName(selectedFriend)}</strong><small>{selectedFriend.online ? selectedFriend.status || "Online" : `Last seen ${formatAge(selectedFriend.lastSeen)}`}</small></span></div>
          <div className="chat-messages" ref={chatMessagesRef} onScroll={onChatScroll} aria-live="polite">{messagesLoading && messages.length === 0 ? <div className="social-loading">Loading conversation…</div> : messages.length === 0 ? <div className="social-empty compact"><MessageSquare size={18} /><strong>Start a conversation</strong><p>Messages are private between confirmed friends.</p></div> : messages.map((item, index) => { const grouped = index > 0 && shouldGroupMessages(messages[index - 1], item); return <article key={item.messageId} className={`chat-message ${item.senderUuid === account.uuid ? "mine" : "theirs"}${grouped ? " grouped" : ""}`}>{!grouped && <small>{item.senderUuid === account.uuid ? "You" : item.senderName} · {formatTime(item.sentAt)}</small>}{renderChatMessage(item.message)}<button type="button" className="chat-copy-button" title="Copy message" aria-label="Copy message" onClick={() => void copyMessage(item.message)}><Copy size={12} /></button></article>; })}{showLatestMessageButton && <button className="chat-latest-button" onClick={jumpToLatest}><ArrowDown size={13} /> Latest</button>}</div>
          {(chatActionError || chatLoadError) && <p className="social-status-message error" role="alert">{chatActionError || chatLoadError}</p>}{chatCopyNotice && <p className="social-status-message success" role="status">{chatCopyNotice}</p>}
          {gifComposerOpen && <section className="gif-picker" aria-label="GIF search">
            <header className="gif-picker-header"><label><Search size={14} /><input autoFocus aria-label="Search GIFs" placeholder="Search GIFs…" maxLength={50} value={gifSearch} onChange={event => setGifSearch(event.target.value)} /></label><button type="button" aria-label="Close GIF search" onClick={() => { setGifComposerOpen(false); setGifUrl(""); setGifSearch(""); }}><X size={15} /></button></header>
            {!GIPHY_API_KEY ? <div className="gif-key-setup"><strong>GIF search is unavailable in this build</strong><small>The launcher needs its shared GIPHY app key configured by the developer. You do not need to enter a personal key. Sending direct GIF links still works below.</small></div> : <>
              {gifError && <p className="gif-picker-error" role="alert">{gifError}</p>}
              <div className="gif-results" aria-live="polite">{gifLoading ? <div className="gif-results-state"><LoaderCircle size={18} className="gif-spinner" />Finding GIFs…</div> : gifResults.length ? gifResults.map(gif => { const previewUrl = gif.images?.fixed_width?.url ?? gif.images?.downsized?.url ?? ""; const candidates = [gif.images?.fixed_width?.url, gif.images?.downsized?.url, gif.images?.original?.url].filter((url): url is string => !!url && isDirectGifUrl(url)); const sendUrl = candidates.sort((left, right) => left.length - right.length).find(url => url.length + "SNINEGIF:".length <= 240) ?? ""; return <button type="button" key={gif.id} className="gif-result" disabled={busy || !sendUrl} title={gif.title || "Send GIF"} onClick={() => void deliverGif(sendUrl, gif.analytics?.onsent?.url)}><img src={previewUrl || sendUrl} alt={gif.title || "GIF preview"} loading="lazy" referrerPolicy="no-referrer" /><span><Send size={12} /> Send</span></button>; }) : <div className="gif-results-state">{gifError ? "Search unavailable" : gifSearch.trim() ? "No GIFs found. Try another search." : "No trending GIFs right now."}</div>}</div>
              <div className="gif-picker-footer"><strong>Powered by GIPHY</strong></div>
            </>}
            <form className="gif-compose" onSubmit={sendGif}><input aria-label="Direct GIF URL" placeholder="Or paste a direct GIPHY / Tenor .gif link" maxLength={512} value={gifUrl} onChange={event => setGifUrl(event.target.value)} /><button aria-label="Send GIF URL" disabled={busy || !gifUrl.trim()}><Send size={14} /></button></form>
          </section>}
          <form className="chat-compose" onSubmit={sendMessage}><button type="button" className="chat-gif-trigger" aria-label="Send a GIF" title="Search and send GIFs" aria-pressed={gifComposerOpen} onClick={() => { setGifComposerOpen(value => !value); setChatActionError(""); }}><ImagePlus size={16} /></button><input autoFocus aria-label={`Message ${displayName(selectedFriend)}`} title="Press Enter to send. Paste a direct GIF URL to preview it." placeholder={`Message ${displayName(selectedFriend)}`} maxLength={240} value={draft} onChange={event => setDraft(event.target.value)} onPaste={handleMessagePaste} /><button aria-label="Send message" disabled={busy || !draft.trim()}>{sendingFriendUuid === selectedFriend.uuid ? <LoaderCircle size={16} className="gif-spinner" /> : <Send size={16} />}</button></form>
        </> : <><div className="side-heading"><strong>Chats</strong><span className="item-count">{snapshot.friends.filter(friend => friend.unreadMessages > 0).length}</span></div>{error && <p className="social-status-message error" role="alert">{error}</p>}{snapshot.friends.length === 0 ? <div className="social-empty"><MessageSquare size={18} /><strong>No conversations</strong><p>Add friends to start a private chat.</p><button className="secondary-action" onClick={() => onTabChange("Friends")}>Find friends</button></div> : <div className="chat-list">{snapshot.friends.slice().sort((a, b) => b.unreadMessages - a.unreadMessages || Number(b.online) - Number(a.online) || displayName(a).localeCompare(displayName(b))).map(friend => <button key={friend.uuid} className="chat-list-row" onClick={() => openChat(friend)}><Avatar name={displayName(friend)} uuid={friend.uuid} /><span><strong>{displayName(friend)}</strong><small>{friend.online ? friend.status || "Online" : "Offline"}</small></span>{friend.unreadMessages > 0 && <i className="unread-count">{friend.unreadMessages > 9 ? "9+" : friend.unreadMessages}</i>}</button>)}</div>}</>}
      </div>}
  </>;
}

function FriendRow({ friend, onChat, onRemove }: { friend: Friend; onChat: () => void; onRemove: () => void }) {
  return <article className="friend-row"><button className="friend-open-chat" onClick={onChat}><Avatar name={displayName(friend)} uuid={friend.uuid} /><span><strong>{displayName(friend)}</strong><small>{friend.online ? friend.status || "Online" : "Offline"}</small></span>{friend.unreadMessages > 0 && <i className="unread-count">{friend.unreadMessages > 9 ? "9+" : friend.unreadMessages}</i>}</button><button className="friend-remove" title="Remove friend" aria-label={`Remove ${displayName(friend)}`} onClick={onRemove}><Trash2 size={13} /></button></article>;
}

function Avatar({ name, uuid }: { name: string; uuid: string }) {
  return <MinecraftHead name={name} uuid={uuid} className="social-avatar" />;
}

function isAllowedGifHost(host: string) { return host === "giphy.com" || host.endsWith(".giphy.com") || host === "tenor.com" || host.endsWith(".tenor.com"); }
function isDirectGifUrl(value: string) { try { const url = new URL(value); return url.protocol === "https:" && url.pathname.toLowerCase().endsWith(".gif") && isAllowedGifHost(url.hostname); } catch { return false; } }
function shouldGroupMessages(previous: FriendMessage, current: FriendMessage) {
  if (previous.senderUuid !== current.senderUuid) return false;
  const previousAt = previous.sentAt < 100000000000 ? previous.sentAt : previous.sentAt / 1000;
  const currentAt = current.sentAt < 100000000000 ? current.sentAt : current.sentAt / 1000;
  const difference = currentAt - previousAt;
  return difference >= 0 && difference <= 5 * 60;
}
async function registerGifSend(url: string) {
  try {
    const tracking = new URL(url);
    if (tracking.protocol !== "https:" || tracking.hostname !== "giphy-analytics.giphy.com") return;
    tracking.searchParams.set("customer_id", "snine-launcher");
    tracking.searchParams.set("ts", String(Math.floor(Date.now() / 1000)));
    await fetch(tracking.toString(), { mode: "no-cors", keepalive: true });
  } catch { /* Analytics must never block sending a message. */ }
}
function displayName(friend: Friend) { return friend.customNickname.trim() || friend.name; }
function formatAge(timestamp: number) { if (!timestamp) return "long ago"; const seconds = Math.max(0, Math.floor(Date.now() / 1000 - timestamp)); return seconds < 60 ? "just now" : seconds < 3600 ? `${Math.floor(seconds / 60)}m ago` : seconds < 86400 ? `${Math.floor(seconds / 3600)}h ago` : `${Math.floor(seconds / 86400)}d ago`; }
function formatTime(timestamp: number) { return new Date(timestamp < 100000000000 ? timestamp * 1000 : timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }); }
function message(reason: unknown) { const text = reason instanceof Error ? reason.message : String(reason); return text.replace(/^Error invoking [^:]+:\s*/, "").replace(/^Error: /, ""); }
