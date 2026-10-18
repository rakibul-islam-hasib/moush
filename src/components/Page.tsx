import { useEffect, useRef, type ReactNode } from "react";

interface PageProps {
  title: string;
  actions?: ReactNode;
  children: ReactNode;
}

/** Page frame with the 28px title. Focus moves to the title when the page opens. */
export function Page({ title, actions, children }: PageProps) {
  const titleRef = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    titleRef.current?.focus({ preventScroll: true });
  }, []);
  return (
    <div className="page">
      <header className="page-header">
        <h1 className="page-title" tabIndex={-1} ref={titleRef}>
          {title}
        </h1>
        {actions && <div className="page-actions">{actions}</div>}
      </header>
      {children}
    </div>
  );
}
