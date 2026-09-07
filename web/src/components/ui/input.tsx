import { cn } from "@/lib/utils";
import { InputHTMLAttributes, forwardRef } from "react";

const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => {
    return (
      <input
        ref={ref}
        className={cn(
          "w-full px-4 py-3 bg-white border-2 border-nb-black rounded-xl text-[13px] font-medium text-nb-black placeholder:text-nb-gray focus:outline-none focus:shadow-neo-yellow focus:border-nb-yellow transition-all",
          className
        )}
        {...props}
      />
    );
  }
);

Input.displayName = "Input";

export { Input };
