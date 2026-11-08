// Stand-in for the GitHub CLI used by scripts/simulate_release_guards.ps1.
//
// The pre-publish guard in .github/workflows/release.yml asks `gh api` whether a
// Release already exists. Reaching the real API from a test would need a token
// and network, so the simulation compiles this stub, puts it on PATH as `gh`,
// and feeds it the answer each scenario wants through GH_STUB_SPEC:
//
//   { "exit_code": 1, "stdout": "", "stderr": "gh: Not Found (HTTP 404)" }
//
// A compiled stub is used rather than a .cmd shim because cmd.exe runs AutoRun
// hooks (this machine has a conda hook), and their console output would land in
// the captured stdout and corrupt the JSON the guard parses.
using System;
using System.IO;
using System.Text;

public static class GhStub
{
    public static int Main(string[] args)
    {
        string specPath = Environment.GetEnvironmentVariable("GH_STUB_SPEC");
        if (string.IsNullOrEmpty(specPath) || !File.Exists(specPath))
        {
            Console.Error.WriteLine("gh stub: no spec at " + specPath);
            return 99;
        }

        string json = File.ReadAllText(specPath);
        if (!string.IsNullOrEmpty(ReadString(json, "stdout"))) Console.Out.Write(ReadString(json, "stdout"));
        if (!string.IsNullOrEmpty(ReadString(json, "stderr"))) Console.Error.Write(ReadString(json, "stderr"));
        return ReadInt(json, "exit_code", 0);
    }

    // The spec is a flat object of three fields, so a full JSON parser is not
    // worth carrying here; this reads exactly what the scenarios write.
    private static string ReadString(string json, string key)
    {
        string marker = "\"" + key + "\":";
        int at = json.IndexOf(marker, StringComparison.Ordinal);
        if (at < 0) return "";
        int i = at + marker.Length;
        while (i < json.Length && char.IsWhiteSpace(json[i])) i++;
        if (i >= json.Length || json[i] != '"') return "";
        i++;
        var sb = new StringBuilder();
        while (i < json.Length)
        {
            char c = json[i];
            if (c == '\\' && i + 1 < json.Length)
            {
                char n = json[i + 1];
                if (n == 'u' && i + 5 < json.Length)
                {
                    sb.Append((char)Convert.ToInt32(json.Substring(i + 2, 4), 16));
                    i += 6;
                    continue;
                }
                switch (n)
                {
                    case 'n': sb.Append('\n'); break;
                    case 'r': sb.Append('\r'); break;
                    case 't': sb.Append('\t'); break;
                    case '"': sb.Append('"'); break;
                    case '\\': sb.Append('\\'); break;
                    default: sb.Append(n); break;
                }
                i += 2;
                continue;
            }
            if (c == '"') break;
            sb.Append(c);
            i++;
        }
        return sb.ToString();
    }

    private static int ReadInt(string json, string key, int fallback)
    {
        string marker = "\"" + key + "\":";
        int at = json.IndexOf(marker, StringComparison.Ordinal);
        if (at < 0) return fallback;
        int i = at + marker.Length;
        while (i < json.Length && char.IsWhiteSpace(json[i])) i++;
        int start = i;
        while (i < json.Length && (char.IsDigit(json[i]) || json[i] == '-')) i++;
        if (i == start) return fallback;
        int value;
        return int.TryParse(json.Substring(start, i - start), out value) ? value : fallback;
    }
}
