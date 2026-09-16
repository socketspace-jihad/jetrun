"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PasswordInput } from "@/components/ui/password-input";
import { Card } from "@/components/ui/card";
import { authApi } from "@/lib/auth";
import { Zap, ArrowRight, Check, Building2, User, Rocket, Loader2 } from "lucide-react";

type Step = "checking" | "org" | "account" | "done";

export default function SetupPage() {
  const router = useRouter();

  const [step, setStep] = useState<Step>("checking");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const [orgName, setOrgName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");

  useEffect(() => {
    authApi
      .setupStatus()
      .then((res) => {
        if (res.setup_completed) {
          router.replace("/login");
        } else {
          setStep("org");
        }
      })
      .catch(() => setStep("org"));
  }, [router]);

  const passwordsMatch = password === confirmPassword;
  const passwordValid = password.length >= 8;
  const canSubmit = email.trim() && passwordValid && passwordsMatch && !loading;

  const handleCreateAll = async () => {
    if (!passwordsMatch) {
      setError("Passwords do not match");
      return;
    }
    setError("");
    setLoading(true);
    try {
      const res = await authApi.initialSetup({
        org_name: orgName,
        email,
        password,
      });

      if (res.access_token) {
        localStorage.setItem("jetrun_token", res.access_token);
        localStorage.setItem("jetrun_refresh_token", res.refresh_token);
      }

      setStep("done");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Setup failed");
    } finally {
      setLoading(false);
    }
  };

  if (step === "checking") {
    return (
      <div className="min-h-screen bg-nb-bg flex items-center justify-center">
        <Loader2 className="w-8 h-8 text-nb-gray animate-spin" />
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-nb-bg flex items-center justify-center p-4">
      <div className="w-full max-w-[480px]">
        {/* Logo */}
        <div className="flex items-center justify-center gap-3 mb-4">
          <div className="w-12 h-12 bg-nb-yellow border-2 border-nb-black rounded-xl shadow-neo flex items-center justify-center">
            <Zap className="w-7 h-7 text-nb-black" />
          </div>
          <h1 className="font-black text-[28px] text-nb-black uppercase tracking-wider">
            jetrun
          </h1>
        </div>
        <p className="text-center text-[13px] text-nb-gray mb-8">
          Welcome! Let&apos;s get your build system running.
        </p>

        {/* Progress */}
        <div className="flex items-center justify-center gap-2 mb-8">
          {[
            { key: "org", label: "Organization" },
            { key: "account", label: "Admin Account" },
            { key: "done", label: "Ready" },
          ].map((s, i) => {
            const isActive = s.key === step;
            const isDone =
              (s.key === "org" && (step === "account" || step === "done")) ||
              (s.key === "account" && step === "done");

            return (
              <div key={s.key} className="flex items-center gap-2">
                {i > 0 && (
                  <div className={`w-8 h-0.5 ${isDone || isActive ? "bg-nb-yellow" : "bg-nb-light"}`} />
                )}
                <div
                  className={`w-7 h-7 rounded-full border-2 flex items-center justify-center text-[10px] font-black transition-all ${
                    isDone
                      ? "bg-nb-yellow border-nb-black text-nb-black"
                      : isActive
                        ? "bg-nb-white border-nb-black text-nb-black"
                        : "bg-nb-bg border-nb-light text-nb-gray"
                  }`}
                >
                  {isDone ? <Check className="w-3.5 h-3.5" /> : i + 1}
                </div>
              </div>
            );
          })}
        </div>

        {/* Step 1: Organization */}
        {step === "org" && (
          <Card className="shadow-neo-lg">
            <div className="flex items-center gap-2 mb-1">
              <Building2 className="w-5 h-5 text-nb-yellow" />
              <h2 className="font-black text-[16px] text-nb-black uppercase tracking-wider">
                Your Organization
              </h2>
            </div>
            <p className="text-[12px] text-nb-gray mb-6">
              Everything in jetrun is scoped to an org — projects, pipelines, users.
            </p>

            <div className="mb-6">
              <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                Organization Name
              </label>
              <Input
                value={orgName}
                onChange={(e) => setOrgName(e.target.value)}
                placeholder="e.g. Acme Corp"
                autoFocus
                onKeyDown={(e) => {
                  if (e.key === "Enter" && orgName.trim()) setStep("account");
                }}
              />
            </div>

            <Button
              className="w-full"
              disabled={!orgName.trim()}
              onClick={() => setStep("account")}
            >
              Continue
              <ArrowRight className="w-4 h-4 ml-2" />
            </Button>
          </Card>
        )}

        {/* Step 2: Admin Account */}
        {step === "account" && (
          <Card className="shadow-neo-lg">
            <div className="flex items-center gap-2 mb-1">
              <User className="w-5 h-5 text-nb-blue" />
              <h2 className="font-black text-[16px] text-nb-black uppercase tracking-wider">
                Admin Account
              </h2>
            </div>
            <p className="text-[12px] text-nb-gray mb-6">
              This will be the first admin of <span className="font-bold text-nb-black">{orgName}</span>.
            </p>

            {error && (
              <div className="bg-nb-red/10 border-2 border-nb-red text-nb-red rounded-xl px-4 py-3 mb-5 text-[12px] font-bold">
                {error}
              </div>
            )}

            <div className="space-y-4 mb-6">
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Email
                </label>
                <Input
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  placeholder="you@company.com"
                  autoFocus
                />
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Password
                </label>
                <PasswordInput
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder="Min 8 characters"
                />
                {password.length > 0 && !passwordValid && (
                  <p className="text-[10px] text-nb-red mt-1 font-bold">At least 8 characters</p>
                )}
              </div>
              <div>
                <label className="block text-[10px] font-black uppercase tracking-widest text-nb-gray mb-2">
                  Confirm Password
                </label>
                <PasswordInput
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  placeholder="Repeat your password"
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && canSubmit) handleCreateAll();
                  }}
                />
                {confirmPassword.length > 0 && !passwordsMatch && (
                  <p className="text-[10px] text-nb-red mt-1 font-bold">Passwords do not match</p>
                )}
              </div>
            </div>

            <div className="flex gap-3">
              <Button variant="secondary" className="flex-1" onClick={() => setStep("org")}>
                Back
              </Button>
              <Button
                className="flex-1"
                disabled={!canSubmit}
                onClick={handleCreateAll}
              >
                {loading ? "Setting up..." : "Create & Launch"}
                {!loading && <Rocket className="w-4 h-4 ml-2" />}
              </Button>
            </div>
          </Card>
        )}

        {/* Step 3: Done */}
        {step === "done" && (
          <Card className="shadow-neo-lg text-center">
            <div className="w-16 h-16 bg-nb-green border-2 border-nb-black rounded-2xl shadow-neo mx-auto mb-5 flex items-center justify-center">
              <Check className="w-9 h-9 text-white" />
            </div>
            <h2 className="font-black text-[20px] text-nb-black uppercase tracking-wider mb-2">
              You&apos;re all set!
            </h2>
            <p className="text-[13px] text-nb-gray mb-6">
              <span className="font-bold text-nb-black">{orgName}</span> is ready.
              Start by creating your first pipeline.
            </p>

            <Button className="w-full" onClick={() => router.push("/")}>
              Go to Dashboard
              <ArrowRight className="w-4 h-4 ml-2" />
            </Button>

            <p className="text-[11px] text-nb-gray mt-4">
              You can invite team members and configure settings later.
            </p>
          </Card>
        )}
      </div>
    </div>
  );
}
