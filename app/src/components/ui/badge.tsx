import type * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded border px-1.5 py-0.5 text-[11px] font-medium leading-none",
  {
    variants: {
      variant: {
        default: "border-border bg-muted text-foreground",
        secondary: "border-transparent bg-muted text-muted-foreground",
        outline: "border-border text-muted-foreground",
        success: "border-border bg-muted text-foreground",
        warning: "border-border bg-muted text-muted-foreground",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

export function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof badgeVariants>) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />;
}
