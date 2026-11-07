# CLI配布の検証と復旧

配布先は `https://d22sm2skny3y0p.cloudfront.net`。CLIの更新はWebサイト `cli.addness.com` の証明書・DNSに依存しない。GitHub Releasesにも同じ実行ファイルとSHA-256を公開する。配布用S3・CloudFrontの識別子はリポジトリ変数 `CLI_S3_BUCKET` / `CLI_CDN_DISTRIBUTION_ID` を参照する。

v0.16.0以前のCLIは更新先が旧ドメインに固定されているため、READMEのCloudFrontインストールコマンドを一度実行して移行する。v0.16.1以降は `addness update` と起動時の自動更新が同じCloudFront配布先を使用する。

CLIは同梱のインストーラへ確認済みの版と現在の実行ファイルの保存先を渡す。インストーラは固定版のアーカイブ・SHA-256・展開した実行ファイルのバージョンを確認してから置換する。更新時にはPATHを変更せず、更新後にもバージョンを確認してから成功表示・再起動する。単独でインストーラを使う場合は `ADDNESS_INSTALL_DIR` で保存先を、`ADDNESS_NO_MODIFY_PATH=1` でPATH変更の省略を指定できる。

## リリース時と日次の確認

Releaseワークフローはアップロード・キャッシュ無効化の後に、利用者と同じ公開URLを検証する。HTTPSが使えない場合やファイル不整合の場合は、リリースジョブを失敗させる。

Distribution healthワークフローは毎日と手動実行にも対応し、次を確認する。

- HTTPSのホスト名・証明書チェーンが有効で、有効期限が14日以上残ること。
- `releases/latest/version.txt` が安定版のタグ形式であること。リリース時には今回のタグと一致すること。
- Unix / Windowsのインストーラが取得できること。
- 4環境の実行ファイルとSHA-256が取得でき、バージョン固定パスとlatestパスが同じ内容であること。

CloudFrontの反映待ちはバージョンが古い場合に限り、最大12回・15秒間隔で行う。TLS検証を無効化しない。日次チェックの失敗はGitHub Actionsの失敗として表示される。通知はリポジトリ利用者のGitHub通知設定に従い、個別のメッセージやIssueを自動投稿しない。

```bash
bash .github/scripts/check-distribution.sh v0.16.1
gh workflow run distribution-health.yml -f expected_version=v0.16.1
```

## 旧Webサイトの証明書を別途復旧する場合

以下は旧ドメイン用の手順で、CLIの更新・リリースの前提条件ではない。

1. 本番AWSに認証し、対象CloudFrontディストリビューションの現在の設定とETagを保存する。
2. 関連付いているACM証明書をus-east-1で確認し、失効理由・検証方法・必要なDNS検証レコードを確認する。
3. DNS検証レコードを正しい値に保つ。期限切れの場合は新しい証明書を要求し、発行完了を確認する。
4. 既存のオリジン・キャッシュ・アクセス制御を維持したまま、CloudFrontの証明書だけを更新する。
5. 反映後に公開URLを検証し、通常のインストールと `addness update --check` を確認する。

AWSのDNS検証レコードは自動更新にも必要なので削除しない。AWSは、DNS検証情報が失われると自動更新できないことと、期限切れ後は新しい証明書を要求することを案内している。[ACMの更新状態](https://docs.aws.amazon.com/acm/latest/userguide/check-certificate-renewal-status.html)、[DNS検証による自動更新](https://docs.aws.amazon.com/acm/latest/userguide/dns-renewal-validation.html)を参照。
