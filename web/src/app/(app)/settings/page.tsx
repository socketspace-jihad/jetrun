"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { formatBytes } from "@/lib/utils";
import { Database, Trash2, Server, Activity } from "lucide-react";
import { DEMO_ENABLED, demoCacheStats, demoServices } from "@/lib/demo";

export default function SettingsPage() {
  const [cacheStats, setCacheStats] = useState(DEMO_ENABLED ? demoCacheStats : null);
  const [services, setServices] = useState(DEMO_ENABLED ? demoServices : []);
  const [loading, setLoading] = useState(!DEMO_ENABLED);
  const [purging, setPurging] = useState(false);

  useEffect(() => {
    if (DEMO_ENABLED) return;

    // Check service health
    const checkServices = async () => {
      const svcList = [
        { name: "gateway", port: 8080 },
        { name: "engine", port: 9001 },
        { name: "worker", port: 9002 },
        { name: "cache", port: 9003 },
        { name: "auth", port: 9004 },
      ];

      const results = await Promise.all(
        svcList.map(async (svc) => {
          try {
            const res = await fetch(`${process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080"}/health`, {
              signal: AbortSignal.timeout(3000),
            });
            return { ...svc, status: res.ok ? "healthy" : "unhealthy", uptime: "—" };
          } catch {
            return { ...svc, status: "unreachable", uptime: "—" };
          }
        })
      );

      // Auth service has its own health check
      try {
        const authRes = await fetch(`${process.env.NEXT_PUBLIC_API_URL || "http://localhost:9004"}/health`, {
          signal: AbortSignal.timeout(3000),
        });
        const authIdx = results.findIndex((s) => s.name === "auth");
        if (authIdx >= 0) {
          results[authIdx].status = authRes.ok ? "healthy" : "unhealthy";
        }
      } catch {}

      setServices(results);
      setLoading(false);
    };

    checkServices();
  }, []);

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">Settings</h1>
        <p className="text-[13px] text-nb-gray mt-1">
          System status and configuration
          {DEMO_ENABLED && <Badge variant="warning" className="ml-2">Demo</Badge>}
        </p>
      </div>

      <div className="grid grid-cols-2 gap-6">
        {/* Cache Stats */}
        <Card>
          <div className="flex items-center justify-between mb-5">
            <CardTitle className="flex items-center gap-2"><Database className="w-4 h-4" />Cache</CardTitle>
            <Button variant="danger" size="sm" disabled={purging} onClick={async () => {
              setPurging(true);
              try {
                const API = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";
                await fetch(`${API}/api/v1/cache`, { method: "DELETE" });
              } catch {}
              setPurging(false);
            }}>
              <Trash2 className="w-3 h-3 mr-1.5" />{purging ? "Purging..." : "Purge"}
            </Button>
          </div>
          <CardContent>
            {cacheStats ? (
              <>
                <div className="grid grid-cols-2 gap-4">
                  <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                    <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">Hit Rate</p>
                    <p className="text-[22px] font-black text-nb-black mt-1">{(cacheStats.hit_rate * 100).toFixed(1)}%</p>
                  </div>
                  <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                    <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">Total Size</p>
                    <p className="text-[22px] font-black text-nb-black mt-1">{formatBytes(cacheStats.total_size_bytes)}</p>
                  </div>
                  <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                    <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">Entries</p>
                    <p className="text-[22px] font-black text-nb-black mt-1">{cacheStats.total_entries.toLocaleString()}</p>
                  </div>
                  <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                    <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">Evictions</p>
                    <p className="text-[22px] font-black text-nb-black mt-1">{cacheStats.eviction_count}</p>
                  </div>
                </div>
                <div className="mt-4 pt-4 border-t border-nb-light">
                  <div className="flex justify-between text-[11px] text-nb-gray mb-2">
                    <span>Hits: <span className="font-bold text-nb-green">{cacheStats.hit_count.toLocaleString()}</span></span>
                    <span>Misses: <span className="font-bold text-nb-red">{cacheStats.miss_count.toLocaleString()}</span></span>
                  </div>
                  <div className="w-full h-2 bg-nb-light rounded-full overflow-hidden">
                    <div className="h-full bg-nb-green rounded-full" style={{ width: `${cacheStats.hit_rate * 100}%` }} />
                  </div>
                </div>
              </>
            ) : (
              <p className="text-[13px] text-nb-gray text-center py-8">Cache stats not available</p>
            )}
          </CardContent>
        </Card>

        {/* Services */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-5"><Server className="w-4 h-4" />Services</CardTitle>
          <CardContent>
            {loading ? (
              <p className="text-[13px] text-nb-gray text-center py-8">Checking services...</p>
            ) : (
              <div className="space-y-3">
                {services.map((svc) => (
                  <div key={svc.name} className="flex items-center justify-between bg-nb-bg border border-nb-light rounded-xl px-4 py-3">
                    <div className="flex items-center gap-3">
                      <div className={`w-2 h-2 rounded-full ${svc.status === "healthy" ? "bg-nb-green" : svc.status === "unhealthy" ? "bg-nb-orange" : "bg-nb-red"}`} />
                      <span className="font-black text-[13px] uppercase tracking-wider">{svc.name}</span>
                    </div>
                    <div className="flex items-center gap-3 text-[11px] text-nb-gray">
                      <span className="font-mono">:{svc.port}</span>
                      <Badge variant={svc.status === "healthy" ? "success" : svc.status === "unhealthy" ? "warning" : "danger"}>
                        {svc.status}
                      </Badge>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
