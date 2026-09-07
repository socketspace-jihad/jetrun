import { cn } from "@/lib/utils";
import { ButtonHTMLAttributes, forwardRef } from "react";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md" | "lg";
}

const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "primary", size = "md", ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          "inline-flex items-center justify-center font-black uppercase tracking-wider border-2 border-nb-black rounded-xl transition-all active:translate-y-0.5 active:shadow-none disabled:opacity-50 disabled:cursor-not-allowed disabled:active:translate-y-0",
          {
            "bg-nb-yellow text-nb-black shadow-neo hover:brightness-105":
              variant === "primary",
            "bg-nb-white text-nb-black shadow-neo hover:bg-nb-bg":
              variant === "secondary",
            "bg-nb-red text-white shadow-neo hover:brightness-105":
              variant === "danger",
            "bg-transparent text-nb-black border-transparent shadow-none hover:bg-nb-bg":
              variant === "ghost",
          },
          {
            "px-3 py-1.5 text-[11px]": size === "sm",
            "px-5 py-2.5 text-[12px]": size === "md",
            "px-7 py-3 text-[13px]": size === "lg",
          },
          className
        )}
        {...props}
      />
    );
  }
);

Button.displayName = "Button";

export { Button };
