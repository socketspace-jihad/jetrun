"use client";

import { cn } from "@/lib/utils";

interface UserAvatarProps {
  name: string | null;
  avatarUrl?: string | null;
  size?: "sm" | "md" | "lg";
  className?: string;
}

const sizeClasses = {
  sm: "w-7 h-7 text-[10px]",
  md: "w-9 h-9 text-[12px]",
  lg: "w-12 h-12 text-[14px]",
};

const colors = [
  "bg-nb-yellow",
  "bg-nb-blue",
  "bg-nb-orange",
  "bg-nb-green",
  "bg-nb-purple",
  "bg-nb-red",
];

function getColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return colors[Math.abs(hash) % colors.length];
}

function getInitials(name: string): string {
  return name
    .split(" ")
    .map((n) => n[0])
    .join("")
    .toUpperCase()
    .slice(0, 2);
}

export function UserAvatar({ name, avatarUrl, size = "md", className }: UserAvatarProps) {
  const displayName = name || "?";

  if (avatarUrl) {
    return (
      <img
        src={avatarUrl}
        alt={displayName}
        className={cn(
          "rounded-xl border-2 border-nb-black object-cover",
          sizeClasses[size],
          className
        )}
      />
    );
  }

  return (
    <div
      className={cn(
        "rounded-xl border-2 border-nb-black flex items-center justify-center font-black text-white",
        sizeClasses[size],
        getColor(displayName),
        className
      )}
    >
      {getInitials(displayName)}
    </div>
  );
}
