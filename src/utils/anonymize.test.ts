import { describe, expect, it } from "vitest";
import { anonymize } from "./anonymize";

const COOKIE =
  "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items.|_CAEaAhADIhwKBGR1aWQSFDEyMzQ1Njc4OTAxMjM0NTY3ODkwKAM.AbCdEfGh12345";
const COOKIE_TAIL = "CAEaAhADIhwKBGR1aWQSFDEyMzQ1Njc4OTAxMjM0NTY3ODkwKAM";

describe("anonymize: cookies and secrets", () => {
  it("removes a bare .ROBLOSECURITY cookie and its warning prefix", () => {
    const out = anonymize(`token was ${COOKIE} end`);
    expect(out).not.toContain(COOKIE_TAIL);
    expect(out).not.toContain("WARNING");
    expect(out).toContain("[cookie]");
    expect(out).toContain("end");
  });

  it("removes the cookie written as a header, as name=value and quoted", () => {
    for (const line of [
      `Cookie: .ROBLOSECURITY=${COOKIE}; path=/`,
      `.ROBLOSECURITY=${COOKIE}`,
      `".ROBLOSECURITY":"${COOKIE}"`,
      `set-cookie: .ROBLOSECURITY=${COOKIE}; domain=.roblox.com`,
    ]) {
      const out = anonymize(line);
      expect(out, line).not.toContain(COOKIE_TAIL);
      expect(out, line).not.toContain("AbCdEfGh12345");
    }
  });

  it("removes a cookie value without the warning prefix (rotated or truncated)", () => {
    const out = anonymize(".ROBLOSECURITY=ABCDEF0123456789short;");
    expect(out).not.toContain("ABCDEF0123456789short");
  });

  it("removes every known secret by its exact value, wherever it appears", () => {
    const out = anonymize("login with hunter2!pass and cookie-xyz-0001 failed", {
      secrets: ["hunter2!pass", "cookie-xyz-0001"],
    });
    expect(out).not.toContain("hunter2!pass");
    expect(out).not.toContain("cookie-xyz-0001");
    expect(out).toContain("failed");
  });

  it("ignores empty or tiny secrets instead of wiping the text", () => {
    expect(anonymize("abc def", { secrets: ["", " ", "a"] })).toBe("abc def");
  });

  it("removes tokens, tickets, csrf, passwords and keys given as fields", () => {
    for (const [line, value] of [
      ["x-csrf-token: AbC123dEf456", "AbC123dEf456"],
      ["RBX-Authentication-Ticket=Zz9_ticket_value", "Zz9_ticket_value"],
      ["authticket: 1234ABCD", "1234ABCD"],
      ["password=Sup3rS3cret", "Sup3rS3cret"],
      ['"privateKey":"9b2f-key-value"', "9b2f-key-value"],
      ["api_key = sk_live_abc123", "sk_live_abc123"],
      ["Authorization: Bearer", "Bearer"],
    ] as const) {
      const out = anonymize(line);
      expect(out, line).not.toContain(value);
      expect(out, line).toContain("[secret]");
    }
  });

  it("removes long opaque tokens even without a field name", () => {
    const token = "a".repeat(20) + "B9".repeat(15) + "==";
    expect(anonymize(`saw ${token} here`)).toBe("saw [token] here");
  });

  it("keeps ordinary error text readable", () => {
    const text = "Failed to get authentication ticket (status 403): Roblox took too long to answer";
    expect(anonymize(text)).toBe(text);
  });
});

describe("anonymize: accounts", () => {
  it("replaces usernames and aliases, longest first and case-insensitively", () => {
    const out = anonymize("ana_alt joined; Ana left; ANA_ALT again; banana stays", {
      accountNames: ["Ana", "ana_alt"],
    });
    expect(out).toBe("[account] joined; [account] left; [account] again; banana stays");
  });

  it("does not treat regex characters in a name as a pattern", () => {
    expect(anonymize("player (x)+ won, pxxx lost", { accountNames: ["(x)+"] })).toBe(
      "player [account] won, pxxx lost"
    );
  });

  it("replaces known user IDs but not longer numbers that contain them", () => {
    const out = anonymize("user 123456789 and place 91234567890 and 123456789", {
      userIds: [123456789],
    });
    expect(out).not.toMatch(/(?<!\d)123456789(?!\d)/);
    expect(out).toContain("91234567890");
  });

  it("replaces user IDs written next to a user label even when unknown", () => {
    for (const line of ["userId=55555", "UserID: 55555", "user 55555", "uid#55555", "user_id = 55555"]) {
      expect(anonymize(line), line).not.toContain("55555");
    }
  });

  it("keeps place IDs, which are public and help debugging", () => {
    expect(anonymize("placeId=2753915549")).toContain("2753915549");
  });
});

describe("anonymize: paths, servers, email and IP", () => {
  it("hides the Windows user name in paths, with either slash", () => {
    for (const path of [
      "C:\\Users\\luanm\\AppData\\Local\\Roblox\\logs\\x.log",
      "c:/Users/Luan Silva/AppData/Local/Roblox Account Manager",
      "D:\\Documents and Settings\\joao\\file.txt",
    ]) {
      const out = anonymize(`at ${path}`);
      expect(out, path).not.toMatch(/luanm|Luan Silva|joao/);
      expect(out, path).toContain("<user>");
    }
  });

  it("keeps the rest of the path so the report still says where", () => {
    expect(anonymize("C:\\Users\\luanm\\AppData\\Local\\Roblox")).toBe("C:\\Users\\<user>\\AppData\\Local\\Roblox");
  });

  it("hides mac and linux home folders", () => {
    expect(anonymize("/Users/maria/Library/Logs and /home/pedro/.config")).toBe(
      "/Users/<user>/Library/Logs and /home/<user>/.config"
    );
  });

  it("hides Job IDs", () => {
    expect(anonymize("job 0f8fad5b-d9cb-469f-a165-70867728950e")).toBe("job [job-id]");
  });

  it("hides private server codes in every form", () => {
    for (const [line, value] of [
      ["vip:12345678901234567890", "12345678901234567890"],
      ["https://www.roblox.com/games/1/x?privateServerLinkCode=987654321", "987654321"],
      ["accessCode=abcd-efgh", "abcd-efgh"],
      ["https://www.roblox.com/share?code=1a2b3c4d&type=Server", "1a2b3c4d"],
      ["launchData=secret-payload", "secret-payload"],
    ] as const) {
      expect(anonymize(line), line).not.toContain(value);
    }
  });

  it("hides e-mail addresses", () => {
    expect(anonymize("mail me at someone.name+tag@example.co.uk now")).toBe("mail me at [email] now");
  });

  it("hides IPv4 (with port) and IPv6 but not version numbers or clock times", () => {
    expect(anonymize("server 128.116.4.20:56789 ok")).toBe("server [ip] ok");
    expect(anonymize("from 2001:db8::8a2e:370:7334")).toBe("from [ip]");
    expect(anonymize("MultiAlt 1.6.0 at 12:34:56")).toBe("MultiAlt 1.6.0 at 12:34:56");
    expect(anonymize("build 10.0.26200.1234")).toBe("build 10.0.26200.1234");
    expect(anonymize("ip fe80:0:0:0:200:f8ff:fe21:67cf%12 x")).toBe("ip [ip] x");
    expect(anonymize("std::vector and a::b")).toBe("std::vector and a::b");
  });
});

describe("anonymize: a realistic log never leaks", () => {
  it("leaves nothing private in a log full of it", () => {
    const secrets = [COOKIE, "MyPassw0rd!"];
    const log = [
      `01:02:03 [info] [auth] Requesting ticket for RealUser42 (userId=987654321)`,
      `01:02:04 [error] [launch] Cookie: .ROBLOSECURITY=${COOKIE}`,
      `01:02:05 [info] [target] Joining 0f8fad5b-d9cb-469f-a165-70867728950e via vip:5555666677778888`,
      `01:02:06 [warn] [build] C:\\Users\\RealWinUser\\AppData\\Local\\Roblox\\Versions missing`,
      `01:02:07 [info] [region] 128.116.4.20 Ashburn`,
      `01:02:08 [error] [login] password=MyPassw0rd! for real.user@example.com`,
    ].join("\n");
    const out = anonymize(log, {
      accountNames: ["RealUser42"],
      userIds: [987654321],
      secrets,
    });
    for (const leak of [
      COOKIE_TAIL,
      "RealUser42",
      "987654321",
      "0f8fad5b",
      "5555666677778888",
      "RealWinUser",
      "128.116.4.20",
      "MyPassw0rd!",
      "real.user@example.com",
    ]) {
      expect(out, leak).not.toContain(leak);
    }
    // O que ajuda a entender o problema continua lá.
    expect(out).toContain("[error] [launch]");
    expect(out).toContain("Ashburn");
    expect(out).toContain("Versions missing");
  });
});
