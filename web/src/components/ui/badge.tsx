import { cn } from "@/lib/utils";
import { HTMLAttributes, forwardRef } from "react";

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: "default" | "success" | "warning" | "danger" | "info" | "muted";
}

const Badge = forwardRef<HTMLSpanElement, BadgeProps>(
  ({ className, variant = "default", ...props }, ref) => {
    return (
      <span
        ref={ref}
        className={cn(
          "inline-flex items-center px-2.5 py-0.5 rounded-full text-[10px] font-black uppercase tracking-widest border",
          {
            "bg-nb-yellow text-nb-black border-nb-black":
              variant === "default",
            "bg-nb-green text-white border-nb-black":
              variant === "success",
            "bg-nb-orange text-white border-nb-black":
              variant === "warning",
            "bg-nb-red text-white border-nb-black":
              variant === "danger",
            "bg-nb-blue text-white border-nb-black":
              variant === "info",
            "bg-nb-light text-nb-gray border-nb-light":
              variant === "muted",
          },
          className
        )}
        {...props}
      />
    );
  }
);

Badge.displayName = "Badge";

export { Badge };
