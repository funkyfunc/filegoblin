import sys
try:
    from reportlab.lib.pagesizes import letter
    from reportlab.platypus import SimpleDocTemplate, Table, TableStyle
    from reportlab.lib import colors
except ImportError:
    print("reportlab not installed. Installing...")
    import subprocess
    subprocess.check_call([sys.executable, "-m", "pip", "install", "reportlab"])
    from reportlab.lib.pagesizes import letter
    from reportlab.platypus import SimpleDocTemplate, Table, TableStyle
    from reportlab.lib import colors

pdf_path = "/Users/stompinggrounds/Development/filegoblin/test_assets/financial_report.pdf"
doc = SimpleDocTemplate(pdf_path, pagesize=letter)

data = [
    ["Date", "Revenue", "Expenses", "Net Profit", "Growth"],
    ["2026-01-01", "$10,000,000", "$8,000,000", "$2,000,000", "+5%"],
    ["2026-02-01", "$12,500,000", "$9,000,000", "$3,500,000", "+20%"],
    ["2026-03-01", "$11,000,000", "$8,500,000", "$2,500,000", "-12%"],
    ["2026-04-01", "$14,000,000", "$9,500,000", "$4,500,000", "+27%"],
]

t = Table(data)
t.setStyle(TableStyle([
    ('BACKGROUND', (0, 0), (-1, 0), colors.grey),
    ('TEXTCOLOR', (0, 0), (-1, 0), colors.whitesmoke),
    ('ALIGN', (0, 0), (-1, -1), 'CENTER'),
    ('FONTNAME', (0, 0), (-1, 0), 'Helvetica-Bold'),
    ('FONTSIZE', (0, 0), (-1, 0), 14),
    ('BOTTOMPADDING', (0, 0), (-1, 0), 12),
    ('BACKGROUND', (0, 1), (-1, -1), colors.beige),
    ('GRID', (0, 0), (-1, -1), 1, colors.black)
]))

elements = [t]
doc.build(elements)
print(f"Generated {pdf_path}")
