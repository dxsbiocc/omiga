"""PDF processing skill for Omiga."""
from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, Dict, List, Optional

from omiga.skills.base import Skill, SkillContext, SkillMetadata, SkillError

logger = logging.getLogger("omiga.skills.pdf")


class PdfSkill(Skill):
    """Skill for processing PDF files."""

    metadata = SkillMetadata(
        name="pdf",
        description="PDF 文件处理 - 读取、提取、合并、分割、创建 PDF",
        emoji="📄",
        tags=["pdf", "document", "file"],
    )

    def __init__(self, context: SkillContext):
        super().__init__(context)

    async def execute(
        self,
        action: str,
        file_path: Optional[str] = None,
        file_paths: Optional[List[str]] = None,
        output_path: Optional[str] = None,
        **kwargs: Any,
    ) -> Any:  # type: ignore[override]
        """Execute the PDF skill.

        Args:
            action: Action to perform (read, extract_tables, merge, split, create, info)
            file_path: Single file path
            file_paths: Multiple file paths for merge
            output_path: Output file path
            **kwargs: Action-specific arguments

        Returns:
            Result of the PDF operation
        """
        actions = {
            "read": self._read_pdf,
            "extract_text": self._extract_text,
            "extract_tables": self._extract_tables,
            "merge": self._merge_pdfs,
            "split": self._split_pdf,
            "info": self._get_pdf_info,
            "count_pages": self._count_pages,
        }

        if action not in actions:
            raise SkillError(f"Unknown action: {action}", self.name)

        return await actions[action](
            file_path=file_path,
            file_paths=file_paths,
            output_path=output_path,
            **kwargs,
        )

    async def _read_pdf(
        self,
        file_path: str,
        pages: Optional[List[int]] = None,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Read text from a PDF file."""
        try:
            from pypdf import PdfReader
        except ImportError:
            raise SkillError("pypdf not installed. Run: pip install pypdf", self.name)

        path = Path(file_path)
        if not path.exists():
            raise SkillError(f"File not found: {file_path}", self.name)

        try:
            reader = PdfReader(str(path))
            text_parts = []

            page_nums = pages or list(range(len(reader.pages)))
            for i in page_nums:
                if 0 <= i < len(reader.pages):
                    page = reader.pages[i]
                    text_parts.append(f"--- Page {i + 1} ---\n{page.extract_text()}")

            return {
                "file": str(path),
                "total_pages": len(reader.pages),
                "pages_read": len(page_nums),
                "text": "\n\n".join(text_parts),
            }
        except Exception as e:
            raise SkillError(f"Failed to read PDF: {e}", self.name)

    async def _extract_text(
        self,
        file_path: str,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Extract all text from a PDF."""
        result = await self._read_pdf(file_path)
        return {
            "file": result["file"],
            "text": result["text"],
        }

    async def _extract_tables(
        self,
        file_path: str,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Extract tables from a PDF."""
        try:
            import pdfplumber
        except ImportError:
            raise SkillError(
                "pdfplumber not installed. Run: pip install pdfplumber", self.name
            )

        path = Path(file_path)
        if not path.exists():
            raise SkillError(f"File not found: {file_path}", self.name)

        try:
            tables_result = []
            with pdfplumber.open(str(path)) as pdf:
                for i, page in enumerate(pdf.pages):
                    tables = page.extract_tables()
                    if tables:
                        for j, table in enumerate(tables):
                            tables_result.append({
                                "page": i + 1,
                                "table_index": j + 1,
                                "data": table,
                                "rows": len(table),
                                "columns": len(table[0]) if table else 0,
                            })

            return {
                "file": str(path),
                "tables_found": len(tables_result),
                "tables": tables_result,
            }
        except Exception as e:
            raise SkillError(f"Failed to extract tables: {e}", self.name)

    async def _merge_pdfs(
        self,
        file_paths: List[str],
        output_path: str,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Merge multiple PDF files."""
        try:
            from pypdf import PdfWriter, PdfReader
        except ImportError:
            raise SkillError("pypdf not installed. Run: pip install pypdf", self.name)

        try:
            writer = PdfWriter()
            merged_files = []

            for fp in file_paths:
                path = Path(fp)
                if not path.exists():
                    raise SkillError(f"File not found: {fp}", self.name)
                reader = PdfReader(str(path))
                for page in reader.pages:
                    writer.add_page(page)
                merged_files.append(str(path))

            output = Path(output_path)
            output.parent.mkdir(parents=True, exist_ok=True)

            with open(output, "wb") as f:
                writer.write(f)

            return {
                "status": "success",
                "merged_files": merged_files,
                "output": str(output),
                "page_count": len(writer.pages),
            }
        except Exception as e:
            raise SkillError(f"Failed to merge PDFs: {e}", self.name)

    async def _split_pdf(
        self,
        file_path: str,
        output_dir: Optional[str] = None,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Split a PDF into individual pages."""
        try:
            from pypdf import PdfWriter, PdfReader
        except ImportError:
            raise SkillError("pypdf not installed. Run: pip install pypdf", self.name)

        path = Path(file_path)
        if not path.exists():
            raise SkillError(f"File not found: {file_path}", self.name)

        output_dir_path = Path(output_dir) if output_dir else path.parent
        output_dir_path.mkdir(parents=True, exist_ok=True)

        try:
            reader = PdfReader(str(path))
            output_files = []

            for i, page in enumerate(reader.pages):
                writer = PdfWriter()
                writer.add_page(page)
                output_file = output_dir_path / f"{path.stem}_page_{i + 1}.pdf"
                with open(output_file, "wb") as f:
                    writer.write(f)
                output_files.append(str(output_file))

            return {
                "status": "success",
                "input": str(path),
                "output_dir": str(output_dir_path),
                "output_files": output_files,
                "page_count": len(output_files),
            }
        except Exception as e:
            raise SkillError(f"Failed to split PDF: {e}", self.name)

    async def _get_pdf_info(
        self,
        file_path: str,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Get PDF metadata."""
        try:
            from pypdf import PdfReader
        except ImportError:
            raise SkillError("pypdf not installed. Run: pip install pypdf", self.name)

        path = Path(file_path)
        if not path.exists():
            raise SkillError(f"File not found: {file_path}", self.name)

        try:
            reader = PdfReader(str(path))
            meta = reader.metadata

            return {
                "file": str(path),
                "page_count": len(reader.pages),
                "title": meta.title if meta else None,
                "author": meta.author if meta else None,
                "subject": meta.subject if meta else None,
                "creator": meta.creator if meta else None,
                "producer": meta.producer if meta else None,
                "file_size_bytes": path.stat().st_size,
            }
        except Exception as e:
            raise SkillError(f"Failed to get PDF info: {e}", self.name)

    async def _count_pages(
        self,
        file_path: str,
        **kwargs: Any,
    ) -> Dict[str, int]:
        """Count pages in a PDF."""
        try:
            from pypdf import PdfReader
        except ImportError:
            raise SkillError("pypdf not installed. Run: pip install pypdf", self.name)

        path = Path(file_path)
        if not path.exists():
            raise SkillError(f"File not found: {file_path}", self.name)

        try:
            reader = PdfReader(str(path))
            return {"file": str(path), "page_count": len(reader.pages)}
        except Exception as e:
            raise SkillError(f"Failed to count pages: {e}", self.name)

    def get_usage(self) -> str:
        """Return usage instructions."""
        return """
PDF Skill - PDF 文件处理

可用操作:
- read <file_path> [pages]: 读取 PDF 文本
- extract_text <file_path>: 提取所有文本
- extract_tables <file_path>: 提取表格
- merge <file_paths> <output_path>: 合并 PDF
- split <file_path> [output_dir]: 分割 PDF
- info <file_path>: 获取 PDF 信息
- count_pages <file_path>: 计算页数

示例:
- 读取 PDF: execute(action="read", file_path="doc.pdf")
- 提取表格：execute(action="extract_tables", file_path="report.pdf")
- 合并 PDF: execute(action="merge", file_paths=["a.pdf", "b.pdf"], output_path="merged.pdf")

需要安装: pip install pypdf pdfplumber
"""
