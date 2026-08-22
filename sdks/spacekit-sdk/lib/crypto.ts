function safeUUID(): string
{
    if ((typeof crypto === "undefined") || (typeof crypto.randomUUID === "undefined")) {
        // TS-safe fallback for iOS WebView (128-bit UUID)
        return "10000000-1000-4000-8000-100000000000".replace(/[018]/g, c => ( Number(c) ^ (crypto.getRandomValues(new Uint8Array(1))[0] & 15 >> Number(c) / 4) ).toString(16) );
    }
    return crypto.randomUUID();
}

export { safeUUID };