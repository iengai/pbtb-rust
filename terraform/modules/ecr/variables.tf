variable "repositories" {
  description = "ECR repositories to manage, keyed by a logical name."
  type = map(object({
    name                 = string
    image_tag_mutability = optional(string, "MUTABLE")
    scan_on_push         = optional(bool, true)
    # force_delete is a Terraform-only behavior (allow destroying a non-empty
    # repo). Default false so a live image repo is never wiped by accident.
    force_delete = optional(bool, false)
  }))
}

variable "tags" {
  description = "Tags applied to every repository."
  type        = map(string)
  default     = {}
}
