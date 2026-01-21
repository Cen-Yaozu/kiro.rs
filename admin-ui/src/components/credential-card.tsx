import { useState } from 'react'
import { toast } from 'sonner'
import { RefreshCw, Wallet, RotateCcw, Trash2 } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { CredentialStatusItem } from '@/types/api'
import {
  useSetDisabled,
  useResetFailure,
  useRefreshToken,
  useDeleteCredential,
} from '@/hooks/use-credentials'

interface CredentialCardProps {
  credential: CredentialStatusItem
  onViewBalance: (id: number) => void
}

function formatAuthMethodLabel(authMethod: string | null): string {
  if (!authMethod) return '未知'
  if (authMethod.toLowerCase() === 'idc') return 'IdC/Builder-ID/IAM'
  return authMethod
}

export function CredentialCard({ credential, onViewBalance }: CredentialCardProps) {
  const [showDeleteDialog, setShowDeleteDialog] = useState(false)

  const setDisabled = useSetDisabled()
  const resetFailure = useResetFailure()
  const refreshToken = useRefreshToken()
  const deleteCredential = useDeleteCredential()

  const handleToggleDisabled = () => {
    setDisabled.mutate(
      { id: credential.id, disabled: !credential.disabled },
      {
        onSuccess: (res) => {
          toast.success(res.message)
        },
        onError: (err) => {
          toast.error('操作失败: ' + (err as Error).message)
        },
      }
    )
  }

  const handleReset = () => {
    resetFailure.mutate(credential.id, {
      onSuccess: (res) => {
        toast.success(res.message)
      },
      onError: (err) => {
        toast.error('操作失败: ' + (err as Error).message)
      },
    })
  }

  const handleRefreshToken = () => {
    refreshToken.mutate(credential.id, {
      onSuccess: (res) => {
        toast.success(res.message)
      },
      onError: (err) => {
        toast.error('刷新失败: ' + (err as Error).message)
      },
    })
  }

  const handleDelete = () => {
    deleteCredential.mutate(credential.id, {
      onSuccess: (res) => {
        toast.success(res.message)
        setShowDeleteDialog(false)
      },
      onError: (err) => {
        toast.error('删除失败: ' + (err as Error).message)
      },
    })
  }

  const formatExpiry = (expiresAt: string | null) => {
    if (!expiresAt) return '未知'
    const date = new Date(expiresAt)
    const now = new Date()
    const diff = date.getTime() - now.getTime()
    if (diff < 0) return '已过期'
    const minutes = Math.floor(diff / 60000)
    if (minutes < 60) return `${minutes} 分钟`
    const hours = Math.floor(minutes / 60)
    if (hours < 24) return `${hours} 小时`
    return `${Math.floor(hours / 24)} 天`
  }

  const isConnectionFull = credential.activeConnections >= credential.maxConcurrent

  return (
    <>
      <Card className={credential.isCurrent ? 'ring-2 ring-primary' : ''}>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle className="text-lg flex items-center gap-2">
              凭据 #{credential.id}
              {credential.isCurrent && (
                <Badge variant="success">当前</Badge>
              )}
              {credential.disabled && (
                <Badge variant="destructive">已禁用</Badge>
              )}
            </CardTitle>
            <div className="flex items-center gap-2">
              <span className="text-sm text-muted-foreground">启用</span>
              <Switch
                checked={!credential.disabled}
                onCheckedChange={handleToggleDisabled}
                disabled={setDisabled.isPending}
              />
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* 信息网格 */}
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div>
              <span className="text-muted-foreground">并发：</span>
              <span className={isConnectionFull ? 'text-orange-500 font-medium' : 'font-medium'}>
                {credential.activeConnections}/{credential.maxConcurrent}
              </span>
            </div>
            <div>
              <span className="text-muted-foreground">失败次数：</span>
              <span className={credential.failureCount > 0 ? 'text-red-500 font-medium' : ''}>
                {credential.failureCount}
              </span>
            </div>
            <div>
              <span className="text-muted-foreground">认证方式：</span>
              <span className="font-medium">{formatAuthMethodLabel(credential.authMethod)}</span>
            </div>
            <div>
              <span className="text-muted-foreground">Token 有效期：</span>
              <span className="font-medium">{formatExpiry(credential.expiresAt)}</span>
            </div>
            {credential.hasProfileArn && (
              <div className="col-span-2">
                <Badge variant="secondary">有 Profile ARN</Badge>
              </div>
            )}
          </div>

          {/* 操作按钮 */}
          <div className="flex flex-wrap gap-2 pt-2 border-t">
            <Button
              size="sm"
              variant="outline"
              onClick={handleRefreshToken}
              disabled={refreshToken.isPending}
            >
              <RotateCcw className="h-4 w-4 mr-1" />
              刷新 Token
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={handleReset}
              disabled={resetFailure.isPending || credential.failureCount === 0}
            >
              <RefreshCw className="h-4 w-4 mr-1" />
              重置失败
            </Button>
            <Button
              size="sm"
              variant="default"
              onClick={() => onViewBalance(credential.id)}
            >
              <Wallet className="h-4 w-4 mr-1" />
              查看余额
            </Button>
            <Button
              size="sm"
              variant="destructive"
              onClick={() => setShowDeleteDialog(true)}
              disabled={!credential.disabled}
              title={!credential.disabled ? '需要先禁用凭据才能删除' : undefined}
            >
              <Trash2 className="h-4 w-4 mr-1" />
              删除
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* 删除确认对话框 */}
      <Dialog open={showDeleteDialog} onOpenChange={setShowDeleteDialog}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>确认删除凭据</DialogTitle>
            <DialogDescription>
              您确定要删除凭据 #{credential.id} 吗？此操作无法撤销。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setShowDeleteDialog(false)}
              disabled={deleteCredential.isPending}
            >
              取消
            </Button>
            <Button
              variant="destructive"
              onClick={handleDelete}
              disabled={deleteCredential.isPending}
            >
              确认删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
