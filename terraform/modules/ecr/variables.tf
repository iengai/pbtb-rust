variable "repositories" {
  description = "ECR repositories to manage, keyed by a logical name."
  type = map(object({
    name                 = string
    image_tag_mutability = optional(string, "MUTABLE")
    scan_on_push         = optional(bool, true)
    # force_delete is a Terraform-only behavior (allow destroying a non-empty
    # repo). Default false so a live image repo is never wiped by accident.
    force_delete = optional(bool, false)

    # Optional expiry. Both unset (the default) means NO lifecycle policy at
    # all: a repo holding live trading images keeps every image until someone
    # removes it deliberately.
    #
    # `keep_last_images` retains the newest N images by push time and expires
    # the rest, tagged ones included. ECR does not know which images a task
    # definition references, so N must stay comfortably above the number of
    # builds between rollouts, and lowering it is destructive -- check what
    # the task definitions point at first.
    keep_last_images = optional(number)
    # Untagged images are layers orphaned when a tag was re-pointed; no task
    # definition can reference them.
    expire_untagged_after_days = optional(number)
  }))

  validation {
    condition = alltrue([
      for r in values(var.repositories) :
      r.keep_last_images == null || try(r.keep_last_images >= 1, false)
    ])
    error_message = "keep_last_images must be at least 1."
  }
}

variable "tags" {
  description = "Tags applied to every repository."
  type        = map(string)
  default     = {}
}
