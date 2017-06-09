<?xml version="1.0"?>

<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0" xmlns:fix="http://www.fixprotocol.org/ns/fast/td/1.1">
  <xsl:output method="text" indent="no" encoding="UTF-8" media-type="application/x-yaml" />

  <xsl:template match="/*">---
    <xsl:apply-templates/>
  </xsl:template>

  <xsl:template match="ThreadGroup">
iterations: <xsl:value-of select="elementProp/intProp[@name='LoopController.loops']"/>
threads: <xsl:value-of select="stringProp[@name='ThreadGroup.num_threads']"/>
plan:</xsl:template>

  <xsl:template match="HTTPSampler">

  - name: <xsl:value-of select="./@testname"/>
    request:
      url: <xsl:choose><xsl:when test="stringProp[@name='HTTPSampler.protocol'] != ''"><xsl:value-of select="stringProp[@name='HTTPSampler.protocol']"/></xsl:when><xsl:otherwise>http</xsl:otherwise></xsl:choose>://<xsl:value-of select="stringProp[@name='HTTPSampler.domain']"/>:80<xsl:value-of select="stringProp[@name='HTTPSampler.port']"/><xsl:value-of select="stringProp[@name='HTTPSampler.path']"/>
      <xsl:if test="stringProp[@name='HTTPSampler.method'] != 'GET'">
      method: <xsl:value-of select="stringProp[@name='HTTPSampler.method']"/></xsl:if>
  </xsl:template>

  <xsl:template match="URLRewritingModifier/*"></xsl:template>
  <xsl:template match="GenericController"></xsl:template>
  <xsl:template match="boolProp"></xsl:template>
  <xsl:template match="elementProp"></xsl:template>
  <xsl:template match="stringProp"></xsl:template>
  <xsl:template match="collectionProp"></xsl:template>

  <xsl:template match="*/text()[normalize-space()]">
    <xsl:value-of select="normalize-space()"/>
  </xsl:template>

  <xsl:strip-space elements="*" />

  <xsl:preserve-space elements="HTTPSampler" />

</xsl:stylesheet>
