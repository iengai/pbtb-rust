# Account-wide guard: no bucket in this account can be made public, whatever a
# future bucket policy or ACL says. Every existing bucket already blocks public
# access individually; this closes the gap for buckets created later or outside
# this repo. Account-level, so it lives here only because this is the account's
# single environment.
resource "aws_s3_account_public_access_block" "this" {
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}
