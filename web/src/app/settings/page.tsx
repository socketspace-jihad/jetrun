"use client";

import { Button } from "@/components/ui/button";
import { Card, CardTitle, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { formatBytes } from "@/lib/utils";
import {
  Database,
  Trash2,
  Server,
  Cpu,
  HardDrive,
  Activity,
} from "lucide-react";

const cacheStats = {
  total_entries: 1247,
  total_size_bytes: 2_147_483_648,
  hit_count: 8934,
  miss_count: 1203,
  hit_rate: 0.881,
  eviction_count: 342,
};

const services = [
  { name: "gateway", status: "healthy", port: 8080, uptime: "3d 14h" },
  { name: "engine", status: "healthy", port: 9001, uptime: "3d 14h" },
  { name: "worker", status: "healthy", port: 9002, uptime: "3d 14h" },
  { name: "cache", status: "healthy", port: 9003, uptime: "3d 14h" },
];

export default function SettingsPage() {
  return (
    <div className="p-8">
      {/* Header */}
      <div className="mb-8">
        <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
          Settings
        </h1>
        <p className="text-[13px] text-nb-gray mt-1">
          System configuration and service status
        </p>
      </div>

      <div className="grid grid-cols-2 gap-6">
        {/* Cache Stats */}
        <Card>
          <div className="flex items-center justify-between mb-5">
            <CardTitle className="flex items-center gap-2">
              <Database className="w-4 h-4" />
              Cache
            </CardTitle>
            <Button variant="danger" size="sm">
              <Trash2 className="w-3 h-3 mr-1.5" />
              Purge
            </Button>
          </div>
          <CardContent>
            <div className="grid grid-cols-2 gap-4">
              <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">
                  Hit Rate
                </p>
                <p className="text-[22px] font-black text-nb-black mt-1">
                  {(cacheStats.hit_rate * 100).toFixed(1)}%
                </p>
              </div>
              <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">
                  Total Size
                </p>
                <p className="text-[22px] font-black text-nb-black mt-1">
                  {formatBytes(cacheStats.total_size_bytes)}
                </p>
              </div>
              <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">
                  Entries
                </p>
                <p className="text-[22px] font-black text-nb-black mt-1">
                  {cacheStats.total_entries.toLocaleString()}
                </p>
              </div>
              <div className="bg-nb-bg border border-nb-light rounded-xl p-3">
                <p className="text-[10px] font-black uppercase tracking-widest text-nb-gray">
                  Evictions
                </p>
                <p className="text-[22px] font-black text-nb-black mt-1">
                  {cacheStats.eviction_count}
                </p>
              </div>
            </div>

            <div className="mt-4 pt-4 border-t border-nb-light">
              <div className="flex justify-between text-[11px] text-nb-gray mb-2">
                <span>
                  Hits: <span className="font-bold text-nb-green">{cacheStats.hit_count.toLocaleString()}</span>
                </span>
                <span>
                  Misses: <span className="font-bold text-nb-red">{cacheStats.miss_count.toLocaleString()}</span>
                </span>
              </div>
              <div className="w-full h-2 bg-nb-light rounded-full overflow-hidden">
                <div
                  className="h-full bg-nb-green rounded-full"
                  style={{ width: `${cacheStats.hit_rate * 100}%` }}
                />
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Services Status */}
        <Card>
          <CardTitle className="flex items-center gap-2 mb-5">
            <Server className="w-4 h-4" />
            Services
          </CardTitle>
          <CardContent>
            <div className="space-y-3">
              {services.map((service) => (
                <div
                  key={service.name}
                  className="flex items-center justify-between bg-nb-bg border border-nb-light rounded-xl px-4 py-3"
                >
                  <div className="flex items-center gap-3">
                    <div className="w-2 h-2 rounded-full bg-nb-green" />
                    <span className="font-black text-[13px] uppercase tracking-wider">
                      {service.name}
                    </span>
                  </div>
                  <div className="flex items-center gap-3 text-[11px] text-nb-gray">
                    <span className="font-mono">:{service.port}</span>
                    <Badge variant="success">{service.status}</Badge>
                    <span>{service.uptime}</span>
                  </div>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>

        {/* General Settings */}
        <Card className="col-span-2">
          <CardTitle className="flex items-center gap-2 mb-5">
            <Cpu className="w-4 h-4" />
            Configuration
          </CardTitle>
          <CardContent>
            <div className="grid grid-cols-2 gap-5">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  API URL
                </label>
                <Input defaultValue="http://localhost:8080" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Max Concurrent Workers
                </label>
                <Input type="number" defaultValue="4" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Cache Max Size
                </label>
                <Input defaultValue="10 GB" />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Artifact Retention (days)
                </label>
                <Input type="number" defaultValue="30" />
              </div>
            </div>
            <div className="mt-5 flex justify-end">
              <Button>Save Settings</Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
