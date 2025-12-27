import { cn } from '@/lib/utils'
import logoDark from '@/assets/logo.png'        // Black logo for light theme
import logoLight from '@/assets/logo-white.png' // White logo for dark theme
import { useTheme } from '@/components/theme-provider'

interface LogoProps {
  variant?: 'full' | 'icon' | 'compact'
  className?: string
  showGlow?: boolean
}

/**
 * Reticle Logo
 * Custom brand logo
 */
export function Logo({ variant = 'full', className, showGlow = false }: LogoProps) {
  const { resolvedTheme } = useTheme()
  const logoSrc = resolvedTheme === 'dark' ? logoLight : logoDark

  if (variant === 'icon') {
    return (
      <div className={cn('relative inline-flex', className)}>
        <img
          src={logoSrc}
          alt="Reticle"
          className={cn(
            'w-8 h-8 object-contain',
            showGlow && 'glow-primary'
          )}
        />
      </div>
    )
  }

  if (variant === 'compact') {
    return (
      <div className={cn('inline-flex items-center gap-2.5', className)}>
        <img
          src={logoSrc}
          alt="Reticle"
          className={cn(
            'w-7 h-7 object-contain',
            showGlow && 'glow-primary'
          )}
        />
        <span className="text-sm font-bold text-foreground tracking-tight">
          Reticle
        </span>
      </div>
    )
  }

  // Full logo with tagline
  return (
    <div className={cn('inline-flex flex-col gap-1', className)}>
      <div className="flex items-center gap-2.5">
        <img
          src={logoSrc}
          alt="Reticle"
          className={cn(
            'w-9 h-9 object-contain',
            showGlow && 'glow-primary'
          )}
        />
        <div>
          <h1 className="text-lg font-bold text-foreground tracking-tight leading-none">
            Reticle
          </h1>
          <p className="text-[10px] text-muted-foreground font-medium tracking-wide uppercase mt-0.5">
            Protocol Inspector
          </p>
        </div>
      </div>
    </div>
  )
}
