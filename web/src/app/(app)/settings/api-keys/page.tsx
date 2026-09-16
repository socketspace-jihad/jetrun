"use client";

import { useState } from "react";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Key, Plus, Copy, Trash2 } from "lucide-react";

const mockKeys = [
  {
    id: "k1",
    name: "CI Pipeline Key",
    prefix: "jr_live_aBcDeFgH",
    scopes: ["build:trigger", "build:read", "pipeline:read"],
    created_at: "2024-02-15T10:00:00Z",
    last_used_at: "2024-03-10T14:30:00Z",
    expires_at: null,
    revoked_at: null,
  },
  {
    id: "k2",
    name: "Read-Only Dashboard",
    prefix: "jr_live_xYzWvUtS",
    scopes: ["build:read", "pipeline:read", "cache:read"],
    created_at: "2024-03-01T10:00:00Z",
    last_used_at: "2024-03-08T09:00:00Z",
    expires_at: "2024-06-01T00:00:00Z",
    revoked_at: null,
  },
  {
    id: "k3",
    name: "Old Key",
    prefix: "jr_live_oLdKeY12",
    scopes: ["build:trigger"],
    created_at: "2024-01-01T10:00:00Z",
    last_used_at: null,
    expires_at: null,
    revoked_at: "2024-02-01T10:00:00Z",
  },
];

export default function ApiKeysPage() {
  const [showCreate, setShowCreate] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");

  return (
    <div className="p-8">
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
            API Keys
          </h1>
          <p className="text-[13px] text-nb-gray mt-1">
            Manage API keys for CI/CD integrations
          </p>
        </div>
        <Button onClick={() => setShowCreate(!showCreate)}>
          <Plus className="w-4 h-4 mr-2" />
          Create Key
        </Button>
      </div>

      {/* Create Key Form */}
      {showCreate && (
        <Card className="mb-6 border-nb-yellow">
          <CardTitle className="flex items-center gap-2 mb-4">
            <Key className="w-4 h-4" />
            New API Key
          </CardTitle>
          <CardContent>
            <div className="flex gap-4">
              <div className="flex-1">
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Key Name
                </label>
                <Input
                  value={newKeyName}
                  onChange={(e) => setNewKeyName(e.target.value)}
                  placeholder="e.g., CI Pipeline Key"
                />
              </div>
              <div className="flex items-end">
                <Button size="md">Generate</Button>
              </div>
            </div>
            <p className="text-[11px] text-nb-gray mt-3">
              The full API key will only be shown once. Save it securely.
            </p>
          </CardContent>
        </Card>
      )}

      {/* Keys List */}
      <Card>
        <CardContent>
          <div className="space-y-3">
            {mockKeys.map((key) => {
              const isRevoked = !!key.revoked_at;
              const isExpired =
                key.expires_at && new Date(key.expires_at) < new Date();

              return (
                <div
                  key={key.id}
                  className={`flex items-center justify-between bg-nb-bg border rounded-xl px-4 py-3 ${
                    isRevoked || isExpired
                      ? "border-nb-light opacity-60"
                      : "border-nb-light"
                  }`}
                >
                  <div className="flex-1">
                    <div className="flex items-center gap-2 mb-1">
                      <span className="font-black text-[13px]">{key.name}</span>
                      {isRevoked && <Badge variant="danger">Revoked</Badge>}
                      {isExpired && !isRevoked && (
                        <Badge variant="warning">Expired</Badge>
                      )}
                      {!isRevoked && !isExpired && (
                        <Badge variant="success">Active</Badge>
                      )}
                    </div>
                    <div className="flex items-center gap-3 text-[11px] text-nb-gray">
                      <code className="font-mono bg-nb-white px-2 py-0.5 rounded border border-nb-light">
                        {key.prefix}...
                      </code>
                      <span>
                        Created:{" "}
                        {new Date(key.created_at).toLocaleDateString()}
                      </span>
                      {key.last_used_at && (
                        <span>
                          Last used:{" "}
                          {new Date(key.last_used_at).toLocaleDateString()}
                        </span>
                      )}
                      {key.expires_at && (
                        <span>
                          Expires:{" "}
                          {new Date(key.expires_at).toLocaleDateString()}
                        </span>
                      )}
                    </div>
                    <div className="flex gap-1 mt-1.5">
                      {key.scopes.map((scope) => (
                        <Badge key={scope} variant="muted" className="text-[8px]">
                          {scope}
                        </Badge>
                      ))}
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    {!isRevoked && (
                      <>
                        <button className="w-8 h-8 rounded-lg flex items-center justify-center text-nb-gray hover:bg-nb-white transition-colors">
                          <Copy className="w-3.5 h-3.5" />
                        </button>
                        <button className="w-8 h-8 rounded-lg flex items-center justify-center text-nb-red hover:bg-nb-red/10 transition-colors">
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      </>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
