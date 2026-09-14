# The showcase CDN: the public pages read the chart bucket's `public/` prefix
# through it (the showcase, and the template catalogue's audience overlay), so
# a showcase or template switch reaches them as soon as its objects are written
# and the short cache expires, with no site rebuild.
#
# Scoped to `public/` twice: the origin path, and the bucket policy's resource.
# Its own file so a scoped apply of it never sweeps in the NAT instance — see
# AGENTS.md + RUNBOOK.md.

locals {
  # The prefixes `ChartConfig` defaults to (src/config/chart.rs); the collector
  # and the API take them from there, and the grants here must name the same.
  chart_public_prefix   = "public"
  chart_showcase_prefix = "showcase"
}

resource "aws_cloudfront_origin_access_control" "showcase" {
  name                              = "${var.project}-${var.env}-showcase"
  description                       = "The showcase CDN's signed reads of the chart bucket"
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

# 30 s unless an object says otherwise, and never more than 60 s: hiding a bot
# takes effect only as fast as a cached copy expires, and nothing invalidates.
resource "aws_cloudfront_cache_policy" "showcase" {
  name        = "${var.project}-${var.env}-showcase"
  min_ttl     = 0
  default_ttl = 30
  max_ttl     = 60

  parameters_in_cache_key_and_forwarded_to_origin {
    cookies_config {
      cookie_behavior = "none"
    }
    headers_config {
      header_behavior = "none"
    }
    query_strings_config {
      query_string_behavior = "none"
    }
    enable_accept_encoding_gzip   = true
    enable_accept_encoding_brotli = true
  }
}

# The console is a page on another origin, so every answer needs the CORS
# header, a 404 included: without it the browser reports a CORS failure where
# the page expects "not public".
resource "aws_cloudfront_response_headers_policy" "showcase" {
  name = "${var.project}-${var.env}-showcase"

  cors_config {
    access_control_allow_credentials = false
    access_control_max_age_sec       = 3600
    origin_override                  = true

    access_control_allow_headers {
      items = ["*"]
    }
    access_control_allow_methods {
      items = ["GET", "HEAD"]
    }
    access_control_allow_origins {
      items = var.web_origins
    }
  }
}

resource "aws_cloudfront_distribution" "showcase" {
  enabled         = true
  comment         = "${var.project}-${var.env} showcase: the chart bucket's public prefix"
  price_class     = "PriceClass_200"
  http_version    = "http2and3"
  is_ipv6_enabled = true

  origin {
    origin_id                = "chart-public"
    domain_name              = module.chart_bucket.bucket_regional_domain_name
    origin_path              = "/${local.chart_public_prefix}"
    origin_access_control_id = aws_cloudfront_origin_access_control.showcase.id
  }

  default_cache_behavior {
    target_origin_id           = "chart-public"
    viewer_protocol_policy     = "redirect-to-https"
    allowed_methods            = ["GET", "HEAD"]
    cached_methods             = ["GET", "HEAD"]
    compress                   = true
    cache_policy_id            = aws_cloudfront_cache_policy.showcase.id
    response_headers_policy_id = aws_cloudfront_response_headers_policy.showcase.id
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    cloudfront_default_certificate = true
  }

  tags = var.common_tags
}

# The chart bucket's one policy. The public-access block stays on: a service
# principal pinned to this distribution by `AWS:SourceArn` is not a public
# grant. `ListBucket` is what makes a missing key a 404 rather than a 403, so
# a real AccessDenied is never read as "not public"; the listing itself cannot
# be reached through the distribution, whose every request sits under the
# origin path and forwards no query string.
resource "aws_s3_bucket_policy" "chart_showcase_cdn" {
  bucket = module.chart_bucket.bucket_name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid       = "ShowcaseCdnReadsPublic"
        Effect    = "Allow"
        Principal = { Service = "cloudfront.amazonaws.com" }
        Action    = ["s3:GetObject"]
        Resource  = "${module.chart_bucket.bucket_arn}/${local.chart_public_prefix}/*"
        Condition = {
          StringEquals = { "AWS:SourceArn" = aws_cloudfront_distribution.showcase.arn }
        }
      },
      {
        Sid       = "ShowcaseCdnTellsAbsence"
        Effect    = "Allow"
        Principal = { Service = "cloudfront.amazonaws.com" }
        Action    = ["s3:ListBucket"]
        Resource  = module.chart_bucket.bucket_arn
        Condition = {
          StringEquals = { "AWS:SourceArn" = aws_cloudfront_distribution.showcase.arn }
        }
      }
    ]
  })
}

output "showcase_cdn_url" {
  description = "Base URL of the showcase CDN, with its trailing slash: the site's VITE_SHOWCASE_URL"
  value       = "https://${aws_cloudfront_distribution.showcase.domain_name}/"
}
